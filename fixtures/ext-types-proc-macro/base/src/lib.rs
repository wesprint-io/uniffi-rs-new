use std::sync::Arc;

#[derive(uniffi::Record)]
struct MyCustomType {
    id: uniffi_ext_types_ffi_types::Id,
}

#[derive(uniffi::Record)]
struct MyCustomTypeUser {
    my_custom_type: MyCustomType,
    id_wrapper: Arc<uniffi_ext_types_ffi_types::IdWrapper>,
}

#[derive(uniffi::Record)]
struct MyCustomTypeBidule {
    my_custom_type: MyCustomType,
    id_wrapper: uniffi_ext_types_ffi_types::IdWrapperContainer,
}

uniffi::setup_scaffolding!();
