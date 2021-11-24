
// Note that we don't yet support `indirect` for enums.
// See https://github.com/mozilla/uniffi-rs/issues/396 for further discussion.
{% import "macros.swift" as swift %}
{%- let e = self.inner() %}
public enum {{ e|type_name }} {
    {% for variant in e.variants() %}
    case {{ variant.name()|enum_variant_swift }}{% if variant.fields().len() > 0 %}({% call swift::field_list_decl(variant) %}){% endif -%}
    {% endfor %}
}

extension {{ e|type_name }}: ViaFfiUsingByteBuffer, ViaFfi {
    fileprivate static func read(from buf: Reader) throws -> {{ e|type_name }} {
        let variant: Int32 = try buf.readInt()
        switch variant {
        {% for variant in e.variants() %}
        case {{ loop.index }}: return .{{ variant.name()|enum_variant_swift }}{% if variant.has_fields() -%}(
            {% for field in variant.fields() -%}
            {{ field.name()|var_name }}: try {{ "buf"|read_var(field) }}{% if loop.last %}{% else %},{% endif %}
            {% endfor -%}
        ){% endif -%}
        {% endfor %}
        default: throw UniffiInternalError.unexpectedEnumCase
        }
    }

    fileprivate func write(into buf: Writer) {
        switch self {
        {% for variant in e.variants() %}
        {% if variant.has_fields() %}
        case let .{{ variant.name()|enum_variant_swift }}({% for field in variant.fields() %}{{ field.name()|var_name }}{%- if loop.last -%}{%- else -%},{%- endif -%}{% endfor %}):
            buf.writeInt(Int32({{ loop.index }}))
            {% for field in variant.fields() -%}
            {{ field.name()|write_var("buf", field) }}
            {% endfor -%}
        {% else %}
        case .{{ variant.name()|enum_variant_swift }}:
            buf.writeInt(Int32({{ loop.index }}))
        {% endif %}
        {%- endfor %}
        }
    }
}

{% if ! self.contains_object_references() %}
extension {{ e|type_name }}: Equatable, Hashable {}
{% endif %}
