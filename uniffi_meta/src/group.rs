/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::*;
use anyhow::{bail, Result};

type MetadataGroupMap = HashMap<String, MetadataGroup>;

// Create empty metadata groups based on the metadata items.
pub fn create_metadata_groups(items: &[Metadata]) -> MetadataGroupMap {
    // Map crate names to MetadataGroup instances
    items
        .iter()
        .filter_map(|i| match i {
            Metadata::Namespace(namespace) => {
                let group = MetadataGroup {
                    namespace: namespace.clone(),
                    namespace_docstring: None,
                    items: BTreeSet::new(),
                };
                Some((namespace.crate_name.clone(), group))
            }
            Metadata::UdlFile(udl) => {
                let namespace = NamespaceMetadata {
                    crate_name: udl.module_path.clone(),
                    name: udl.namespace.clone(),
                };
                let group = MetadataGroup {
                    namespace,
                    namespace_docstring: None,
                    items: BTreeSet::new(),
                };
                Some((udl.module_path.clone(), group))
            }
            _ => None,
        })
        .collect::<HashMap<_, _>>()
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct ItemIdentifier {
    module_path: String,
    name: String,
}

impl ItemIdentifier {
    pub fn new(module_path: String, name: String) -> Self {
        Self { module_path, name }
    }
}

/// Returns whether an item contains object references.
fn item_contains_references(
    items_map: &HashMap<ItemIdentifier, &Metadata>,
    items_without_references: &mut HashSet<ItemIdentifier>,
    item_id: ItemIdentifier,
) -> bool {
    // Already computed
    if items_without_references.contains(&item_id) {
        return false;
    }

    // New item
    let Some(item) = items_map.get(&item_id).copied() else {
        return true;
    };

    let contains_object_references_fn = |ty: &Type| match ty {
        Type::Object { .. } => true,
        Type::External {
            ref module_path,
            ref name,
            ..
        }
        | Type::Record {
            ref module_path,
            ref name,
            ..
        }
        | Type::Enum {
            ref module_path,
            ref name,
            ..
        } => item_contains_references(
            items_map,
            items_without_references,
            ItemIdentifier::new(module_path.clone(), name.clone()),
        ),

        _ => false,
    };

    let contains_object_references = match item {
        Metadata::Record(meta) => meta
            .fields
            .iter()
            .flat_map(|field| field.ty.iter_types())
            .any(contains_object_references_fn),
        Metadata::Enum(meta) => meta
            .variants
            .iter()
            .flat_map(|v| &v.fields)
            .flat_map(|field| field.ty.iter_types())
            .any(contains_object_references_fn),
        _ => return true,
    };

    if !contains_object_references {
        items_without_references.insert(item_id);
    }

    contains_object_references
}

/// Computes a set of items that **do not** contain object references.
pub fn compute_types_without_obj_refs(items: &[Metadata]) -> HashSet<ItemIdentifier> {
    // Construct a map of all the metadata items
    let items_map = items
        .iter()
        .map(extract_item_identifier)
        .zip(items)
        .filter_map(|(i, j)| i.zip(Some(j)))
        .collect();

    let mut result = HashSet::new();

    // Populate the set with items that have no object references
    for item in items.iter().filter_map(extract_item_identifier) {
        item_contains_references(&items_map, &mut result, item);
    }
    result
}

fn extract_item_identifier(metadata: &Metadata) -> Option<ItemIdentifier> {
    match metadata {
        Metadata::Record(RecordMetadata {
            module_path, name, ..
        })
        | Metadata::Enum(EnumMetadata {
            module_path, name, ..
        }) => Some(ItemIdentifier::new(module_path.clone(), name.clone())),
        _ => None,
    }
}

/// Consume the items into the previously created metadata groups.
pub fn group_metadata(
    group_map: &mut MetadataGroupMap,
    items: Vec<Metadata>,
    items_without_obj_refs: &HashSet<ItemIdentifier>,
) -> Result<()> {
    for item in items {
        if matches!(&item, Metadata::Namespace(_)) {
            continue;
        }

        let crate_name = calc_crate_name(item.module_path()).to_owned(); // XXX - kill clone?

        let item = fixup_external_type(item, group_map, &items_without_obj_refs);
        let group = match group_map.get_mut(&crate_name) {
            Some(ns) => ns,
            None => bail!("Unknown namespace for {item:?} ({crate_name})"),
        };
        if group.items.contains(&item) {
            bail!("Duplicate metadata item: {item:?}");
        }
        group.add_item(item);
    }
    Ok(())
}

#[derive(Debug)]
pub struct MetadataGroup {
    pub namespace: NamespaceMetadata,
    pub namespace_docstring: Option<String>,
    pub items: BTreeSet<Metadata>,
}

impl MetadataGroup {
    pub fn add_item(&mut self, item: Metadata) {
        self.items.insert(item);
    }
}

pub fn fixup_external_type(
    item: Metadata,
    group_map: &MetadataGroupMap,
    items_without_obj_refs: &HashSet<ItemIdentifier>,
) -> Metadata {
    let crate_name = calc_crate_name(item.module_path()).to_owned();
    let converter = ExternalTypeConverter {
        crate_name: &crate_name,
        crate_to_namespace: group_map,
        items_without_obj_refs,
    };
    converter.convert_item(item)
}

/// Convert metadata items by replacing types from external crates with Type::External
struct ExternalTypeConverter<'a> {
    crate_name: &'a str,
    crate_to_namespace: &'a MetadataGroupMap,
    items_without_obj_refs: &'a HashSet<ItemIdentifier>,
}

impl<'a> ExternalTypeConverter<'a> {
    fn crate_to_namespace(&self, crate_name: &str) -> String {
        self.crate_to_namespace
            .get(crate_name)
            .unwrap_or_else(|| panic!("Can't find namespace for module {crate_name}"))
            .namespace
            .name
            .clone()
    }

    fn convert_item(&self, item: Metadata) -> Metadata {
        match item {
            Metadata::Func(meta) => Metadata::Func(FnMetadata {
                inputs: self.convert_params(meta.inputs),
                return_type: self.convert_optional(meta.return_type),
                throws: self.convert_optional(meta.throws),
                ..meta
            }),
            Metadata::Method(meta) => Metadata::Method(MethodMetadata {
                inputs: self.convert_params(meta.inputs),
                return_type: self.convert_optional(meta.return_type),
                throws: self.convert_optional(meta.throws),
                ..meta
            }),
            Metadata::TraitMethod(meta) => Metadata::TraitMethod(TraitMethodMetadata {
                inputs: self.convert_params(meta.inputs),
                return_type: self.convert_optional(meta.return_type),
                throws: self.convert_optional(meta.throws),
                ..meta
            }),
            Metadata::Constructor(meta) => Metadata::Constructor(ConstructorMetadata {
                inputs: self.convert_params(meta.inputs),
                throws: self.convert_optional(meta.throws),
                ..meta
            }),
            Metadata::Record(meta) => Metadata::Record(RecordMetadata {
                fields: self.convert_fields(meta.fields),
                ..meta
            }),
            Metadata::Enum(meta) => Metadata::Enum(self.convert_enum(meta)),
            _ => item,
        }
    }

    fn convert_params(&self, params: Vec<FnParamMetadata>) -> Vec<FnParamMetadata> {
        params
            .into_iter()
            .map(|param| FnParamMetadata {
                ty: self.convert_type(param.ty),
                ..param
            })
            .collect()
    }

    fn convert_fields(&self, fields: Vec<FieldMetadata>) -> Vec<FieldMetadata> {
        fields
            .into_iter()
            .map(|field| FieldMetadata {
                ty: self.convert_type(field.ty),
                ..field
            })
            .collect()
    }

    fn convert_enum(&self, enum_: EnumMetadata) -> EnumMetadata {
        EnumMetadata {
            variants: enum_
                .variants
                .into_iter()
                .map(|variant| VariantMetadata {
                    fields: self.convert_fields(variant.fields),
                    ..variant
                })
                .collect(),
            ..enum_
        }
    }

    fn convert_optional(&self, ty: Option<Type>) -> Option<Type> {
        ty.map(|ty| self.convert_type(ty))
    }

    fn convert_type(&self, ty: Type) -> Type {
        match ty {
            // Convert `ty` if it's external
            Type::Enum { module_path, name } | Type::Record { module_path, name }
                if self.is_module_path_external(&module_path) =>
            {
                let contains_object_references = !self
                    .items_without_obj_refs
                    .contains(&ItemIdentifier::new(module_path.clone(), name.clone()));

                Type::External {
                    namespace: self.crate_to_namespace(&module_path),
                    module_path,
                    name,
                    kind: ExternalKind::DataClass,
                    tagged: false,
                    contains_object_references,
                }
            }
            Type::Custom {
                module_path, name, ..
            } if self.is_module_path_external(&module_path) => {
                // For now, it's safe to assume that all custom types are data classes.
                // There's no reason to use a custom type with an interface.
                Type::External {
                    namespace: self.crate_to_namespace(&module_path),
                    module_path,
                    name,
                    kind: ExternalKind::DataClass,
                    tagged: false,
                    contains_object_references: true,
                }
            }
            Type::Object {
                module_path, name, ..
            } if self.is_module_path_external(&module_path) => Type::External {
                namespace: self.crate_to_namespace(&module_path),
                module_path,
                name,
                kind: ExternalKind::Interface,
                tagged: false,
                contains_object_references: true,
            },
            Type::CallbackInterface { module_path, name }
                if self.is_module_path_external(&module_path) =>
            {
                panic!("External callback interfaces not supported ({name})")
            }
            // Convert child types
            Type::Custom {
                module_path,
                name,
                builtin,
                ..
            } => Type::Custom {
                module_path,
                name,
                builtin: Box::new(self.convert_type(*builtin)),
            },
            Type::Optional { inner_type } => Type::Optional {
                inner_type: Box::new(self.convert_type(*inner_type)),
            },
            Type::Sequence { inner_type } => Type::Sequence {
                inner_type: Box::new(self.convert_type(*inner_type)),
            },
            Type::Map {
                key_type,
                value_type,
            } => Type::Map {
                key_type: Box::new(self.convert_type(*key_type)),
                value_type: Box::new(self.convert_type(*value_type)),
            },
            // Existing External types probably need namespace fixed.
            Type::External {
                namespace,
                module_path,
                name,
                kind,
                tagged,
                contains_object_references,
            } => {
                assert!(namespace.is_empty());
                Type::External {
                    namespace: self.crate_to_namespace(&module_path),
                    module_path,
                    name,
                    kind,
                    tagged,
                    contains_object_references,
                }
            }

            // Otherwise, just return the type unchanged
            _ => ty,
        }
    }

    fn is_module_path_external(&self, module_path: &str) -> bool {
        calc_crate_name(module_path) != self.crate_name
    }
}

fn calc_crate_name(module_path: &str) -> &str {
    module_path.split("::").next().unwrap()
}
