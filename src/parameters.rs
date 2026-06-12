use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::{OnshapeSource, ParameterOverride};

pub const SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterSchema {
    pub schema_version: u32,
    pub source: OnshapeSource,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    pub kind: ParameterKind,
    pub required: bool,
    pub default_value: Option<String>,
    pub options: Vec<ParameterOption>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility_condition: Option<ParameterVisibilityCondition>,
    #[serde(default)]
    pub precision: Option<u32>,
    #[serde(default)]
    pub widget: Option<String>,
    #[serde(default)]
    pub units: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    Text,
    Number,
    Boolean,
    Enum,
    Unsupported,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ParameterVisibilityCondition {
    All {
        conditions: Vec<ParameterVisibilityCondition>,
    },
    Any {
        conditions: Vec<ParameterVisibilityCondition>,
    },
    Equal {
        parameter_id: String,
        values: Vec<String>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValidatedConfiguration {
    pub values: HashMap<String, String>,
    pub typed_values: BTreeMap<String, CanonicalParameterValue>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CanonicalParameterValue {
    Text {
        value: String,
    },
    Number {
        expression: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        units: Option<String>,
    },
    Boolean {
        value: bool,
    },
    Enum {
        value: String,
    },
}

pub fn encoding_request_values(
    typed_values: &BTreeMap<String, CanonicalParameterValue>,
) -> BTreeMap<String, String> {
    typed_values
        .iter()
        .map(|(parameter_id, value)| {
            let request_value = match value {
                CanonicalParameterValue::Text { value }
                | CanonicalParameterValue::Enum { value } => value.clone(),
                CanonicalParameterValue::Boolean { value } => value.to_string(),
                CanonicalParameterValue::Number { expression, units } => {
                    canonical_request_number_value(expression, units.as_deref())
                }
            };
            (parameter_id.clone(), request_value)
        })
        .collect()
}

pub fn normalize_configuration(source: &OnshapeSource, raw: &Value) -> ParameterSchema {
    let parameters = find_parameter_array(raw)
        .map(|items| {
            items
                .iter()
                .filter_map(normalize_parameter)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    ParameterSchema {
        schema_version: SCHEMA_VERSION,
        source: source.clone(),
        parameters,
    }
}

pub fn apply_overrides(
    schema: &mut ParameterSchema,
    overrides: &HashMap<String, ParameterOverride>,
) {
    if overrides.is_empty() {
        return;
    }

    for parameter in &mut schema.parameters {
        let Some(override_) = overrides.get(&parameter.id) else {
            continue;
        };
        if let Some(label) = &override_.label {
            parameter.label = label.clone();
        }
        if override_.description.is_some() {
            parameter.description = override_.description.clone();
        }
        if override_.hidden {
            parameter.hidden = true;
        }
        if override_.precision.is_some() {
            parameter.precision = override_.precision;
        }
        if override_.widget.is_some() {
            parameter.widget = override_.widget.clone();
        }
    }
}

pub fn validate_values(
    schema: &ParameterSchema,
    submitted: &HashMap<String, String>,
    allow_unknown: bool,
) -> Result<ValidatedConfiguration, Vec<String>> {
    let known = schema
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<HashSet<_>>();
    let mut errors = Vec::new();

    if !allow_unknown {
        for name in submitted.keys() {
            if !known.contains(name.as_str()) {
                errors.push(format!("unknown parameter: {name}"));
            }
        }
    }

    let mut values = HashMap::new();
    let mut typed_values = BTreeMap::new();
    for parameter in &schema.parameters {
        if parameter.kind == ParameterKind::Unsupported {
            errors.push(unsupported_parameter_message(parameter));
            continue;
        }

        let value = submitted
            .get(&parameter.id)
            .filter(|value| !value.is_empty())
            .or(parameter.default_value.as_ref());

        match value {
            Some(value) => match parameter.kind {
                ParameterKind::Text => {
                    values.insert(parameter.id.clone(), value.clone());
                    typed_values.insert(
                        parameter.id.clone(),
                        CanonicalParameterValue::Text {
                            value: value.clone(),
                        },
                    );
                }
                ParameterKind::Number => {
                    let canonical = if parameter.units.is_some() {
                        canonicalize_dimensioned_number(parameter, value)
                    } else {
                        canonical_number_string(value).map(|expression| {
                            CanonicalParameterValue::Number {
                                expression,
                                units: None,
                            }
                        })
                    };
                    if let Some(canonical) = canonical {
                        let request_value = if parameter.units.is_some() {
                            number_request_value(parameter, value)
                        } else {
                            value.clone()
                        };
                        values.insert(parameter.id.clone(), request_value);
                        typed_values.insert(parameter.id.clone(), canonical);
                    } else {
                        errors.push(format!("{} must be a number", parameter.label));
                    }
                }
                ParameterKind::Boolean => match value.as_str() {
                    "true" | "false" | "on" | "0" | "1" => {
                        let normalized = normalize_bool(value);
                        values.insert(parameter.id.clone(), normalized.to_owned());
                        typed_values.insert(
                            parameter.id.clone(),
                            CanonicalParameterValue::Boolean {
                                value: normalized == "true",
                            },
                        );
                    }
                    _ => errors.push(format!("{} must be a boolean", parameter.label)),
                },
                ParameterKind::Enum => {
                    if parameter
                        .options
                        .iter()
                        .any(|option| option.value == *value)
                    {
                        values.insert(parameter.id.clone(), value.clone());
                        typed_values.insert(
                            parameter.id.clone(),
                            CanonicalParameterValue::Enum {
                                value: value.clone(),
                            },
                        );
                    } else {
                        errors.push(format!("{} has an invalid option", parameter.label));
                    }
                }
                ParameterKind::Unsupported => unreachable!("handled before value lookup"),
            },
            None if parameter.required => errors.push(format!("{} is required", parameter.label)),
            None => {}
        }
    }

    if allow_unknown {
        for (name, value) in submitted {
            if known.contains(name.as_str()) || value.is_empty() {
                continue;
            }

            values.insert(name.clone(), value.clone());
            typed_values.insert(
                name.clone(),
                CanonicalParameterValue::Text {
                    value: value.clone(),
                },
            );
        }
    }

    if errors.is_empty() {
        Ok(ValidatedConfiguration {
            values,
            typed_values,
        })
    } else {
        Err(errors)
    }
}

fn normalize_parameter(value: &Value) -> Option<Parameter> {
    let object = value.as_object()?;
    let message = object
        .get("message")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let id = first_string(message, &["parameterId", "id", "messageId"])?;
    let label =
        first_string(message, &["parameterName", "name", "label"]).unwrap_or_else(|| id.clone());
    let type_hint = first_text(object, &["typeName", "parameterType", "type"])
        .or_else(|| {
            first_text(
                message,
                &["typeName", "parameterType", "quantityType", "type"],
            )
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_integer = type_hint.contains("integer")
        || first_text(message, &["quantityType"])
            .is_some_and(|quantity_type| quantity_type == "INTEGER");
    let options = extract_options(value);
    let default_value = normalize_default_value(
        message
            .get("defaultValue")
            .or_else(|| message.get("default"))
            .or_else(|| message.get("value"))
            .or_else(|| {
                range_and_default_message(message).and_then(|value| value.get("defaultValue"))
            }),
        is_integer,
    );
    let units = range_and_default_message(message)
        .and_then(|value| value.get("units"))
        .and_then(value_to_string)
        .filter(|units| !units.is_empty());
    let kind = if !options.is_empty() {
        ParameterKind::Enum
    } else if type_hint.contains("bool") || object.values().any(Value::is_boolean) {
        ParameterKind::Boolean
    } else if type_hint.contains("double")
        || type_hint.contains("number")
        || type_hint.contains("integer")
        || type_hint.contains("real")
        || type_hint.contains("length")
        || type_hint.contains("angle")
        || default_value
            .as_deref()
            .is_some_and(|value| value.parse::<f64>().is_ok())
    {
        ParameterKind::Number
    } else if type_hint.contains("string") || type_hint.contains("text") {
        ParameterKind::Text
    } else {
        ParameterKind::Unsupported
    };

    Some(Parameter {
        id,
        label,
        description: first_string(message, &["description", "helpText"]),
        kind,
        required: message
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        default_value,
        options,
        hidden: false,
        visibility_condition: message
            .get("visibilityCondition")
            .or_else(|| object.get("visibilityCondition"))
            .and_then(normalize_visibility_condition),
        precision: is_integer.then_some(0),
        widget: None,
        units,
        raw: value.clone(),
    })
}

impl Parameter {
    pub fn display_value(&self) -> Option<String> {
        self.default_value
            .as_deref()
            .map(|value| self.configuration_value(value))
    }

    fn configuration_value(&self, value: &str) -> String {
        if value.parse::<f64>().is_err() {
            return value.to_owned();
        }

        match self.units.as_deref().and_then(onshape_unit_suffix) {
            Some(unit) => format!("{value} {unit}"),
            None => value.to_owned(),
        }
    }
}

fn canonicalize_dimensioned_number(
    parameter: &Parameter,
    value: &str,
) -> Option<CanonicalParameterValue> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let expression = if let Some(number) = canonical_number_string(trimmed) {
        parameter.configuration_value(&number)
    } else if let Some(expression) = canonical_quantity_expression(trimmed) {
        expression
    } else {
        return None;
    };

    Some(CanonicalParameterValue::Number {
        expression,
        units: parameter.units.clone(),
    })
}

fn canonical_number_string(value: &str) -> Option<String> {
    let number = value.trim().parse::<f64>().ok()?;
    if !number.is_finite() {
        return None;
    }
    Some(number.to_string())
}

fn canonical_request_number_value(expression: &str, units: Option<&str>) -> String {
    let trimmed = expression.trim();
    if let Some(number) = canonical_number_string(trimmed) {
        return request_number_with_default_units(&number, units);
    }

    if let Some(expression) = canonical_quantity_expression(trimmed) {
        return expression;
    }

    trimmed.to_owned()
}

fn canonical_quantity_expression(value: &str) -> Option<String> {
    let (unit_start, unit) = trailing_quantity_unit(value)?;
    let number = canonical_number_string(value[..unit_start].trim())?;
    let unit = canonical_quantity_unit(unit)?;
    Some(format!("{number} {unit}"))
}

fn canonical_quantity_unit(unit: &str) -> Option<&'static str> {
    match unit {
        "mm" | "millimeter" => Some("mm"),
        "cm" | "centimeter" => Some("cm"),
        "m" | "meter" => Some("m"),
        "in" | "inch" => Some("in"),
        "ft" | "foot" => Some("ft"),
        "deg" | "degree" => Some("deg"),
        "rad" | "radian" => Some("rad"),
        _ => None,
    }
}

fn unsupported_parameter_message(parameter: &Parameter) -> String {
    format!(
        "{} ({}) uses an unsupported parameter type",
        parameter.label, parameter.id
    )
}

fn request_number_with_default_units(number: &str, units: Option<&str>) -> String {
    match units.and_then(onshape_unit_suffix) {
        Some(unit) => format!("{number} {unit}"),
        None => number.to_owned(),
    }
}

fn number_request_value(parameter: &Parameter, value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.parse::<f64>().is_ok() {
        request_number_with_default_units(trimmed, parameter.units.as_deref())
    } else if let Some(expression) = canonical_quantity_expression(trimmed) {
        expression
    } else {
        trimmed.to_owned()
    }
}

fn trailing_quantity_unit(value: &str) -> Option<(usize, &str)> {
    let end = value.trim_end().len();
    let trimmed = &value[..end];
    let start = trimmed
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_ascii_alphabetic())
        .map(|(index, character)| index + character.len_utf8())
        .unwrap_or(0);
    let unit = &trimmed[start..];
    (!unit.is_empty()).then_some((start, unit))
}

fn range_and_default_message(message: &serde_json::Map<String, Value>) -> Option<&Value> {
    message
        .get("rangeAndDefault")
        .and_then(|value| value.get("message"))
}

fn onshape_unit_suffix(units: &str) -> Option<&'static str> {
    match units {
        "millimeter" => Some("mm"),
        "centimeter" => Some("cm"),
        "meter" => Some("m"),
        "inch" => Some("in"),
        "foot" => Some("ft"),
        "degree" => Some("deg"),
        "radian" => Some("rad"),
        _ => None,
    }
}

fn find_parameter_array(value: &Value) -> Option<&Vec<Value>> {
    if let Some(array) = value
        .get("configurationParameters")
        .or_else(|| value.get("parameters"))
        .and_then(Value::as_array)
    {
        return Some(array);
    }

    value.as_object()?.values().find_map(find_parameter_array)
}

fn extract_options(value: &Value) -> Vec<ParameterOption> {
    let arrays = value.as_object().into_iter().flat_map(|object| {
        let message = object.get("message").and_then(Value::as_object);
        [
            object.get("options"),
            object.get("items"),
            message.and_then(|message| message.get("options")),
            message.and_then(|message| message.get("items")),
        ]
    });

    arrays
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|item| {
            if let Some(text) = value_to_string(item) {
                return Some(ParameterOption {
                    value: text.clone(),
                    label: text,
                });
            }

            let object = item.as_object()?;
            let message = object
                .get("message")
                .and_then(Value::as_object)
                .unwrap_or(object);
            let value = first_string(message, &["option", "value", "id", "message"])?;
            let label = first_string(message, &["optionName", "label", "name", "displayName"])
                .unwrap_or_else(|| value.clone());
            Some(ParameterOption { value, label })
        })
        .collect()
}

fn normalize_visibility_condition(value: &Value) -> Option<ParameterVisibilityCondition> {
    normalize_visibility_condition_result(value).ok().flatten()
}

fn normalize_visibility_condition_result(
    value: &Value,
) -> Result<Option<ParameterVisibilityCondition>, ()> {
    let object = value.as_object().ok_or(())?;
    let message = object
        .get("message")
        .and_then(Value::as_object)
        .unwrap_or(object);

    if message.is_empty() {
        return Ok(None);
    }

    let type_hint = first_text(object, &["typeName", "type"])
        .or_else(|| first_text(message, &["typeName", "type"]))
        .unwrap_or_default()
        .to_ascii_lowercase();

    if type_hint.contains("visibilitylogical")
        || (message.contains_key("operation") && message.contains_key("children"))
    {
        return normalize_logical_visibility_condition(message);
    }

    if type_hint.contains("visibilityonequal")
        || (message.contains_key("parameterId") && message.contains_key("value"))
    {
        return normalize_equal_visibility_condition(message);
    }

    Err(())
}

fn normalize_logical_visibility_condition(
    message: &serde_json::Map<String, Value>,
) -> Result<Option<ParameterVisibilityCondition>, ()> {
    let operation = first_text(message, &["operation"])
        .ok_or(())?
        .to_ascii_uppercase();
    let children = message
        .get("children")
        .and_then(Value::as_array)
        .ok_or(())?;
    let mut conditions = Vec::new();

    for child in children {
        match normalize_visibility_condition_result(child)? {
            Some(condition) => conditions.push(condition),
            None if operation == "OR" => return Ok(None),
            None => {}
        }
    }

    if conditions.is_empty() {
        return Ok(None);
    }

    match operation.as_str() {
        "AND" => Ok(Some(ParameterVisibilityCondition::All { conditions })),
        "OR" => Ok(Some(ParameterVisibilityCondition::Any { conditions })),
        _ => Err(()),
    }
}

fn normalize_equal_visibility_condition(
    message: &serde_json::Map<String, Value>,
) -> Result<Option<ParameterVisibilityCondition>, ()> {
    let parameter_id = first_string(message, &["parameterId"]).ok_or(())?;
    let value = message.get("value").ok_or(())?;
    let in_array = message
        .get("inArray")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let values = visibility_condition_values(value, in_array).ok_or(())?;

    Ok(Some(ParameterVisibilityCondition::Equal {
        parameter_id,
        values,
    }))
}

fn visibility_condition_values(value: &Value, in_array: bool) -> Option<Vec<String>> {
    let values = if in_array {
        if let Some(array) = value.as_array() {
            array
                .iter()
                .map(value_to_string)
                .collect::<Option<Vec<_>>>()?
        } else {
            vec![value_to_string(value)?]
        }
    } else {
        vec![value_to_string(value)?]
    };

    (!values.is_empty()).then_some(values)
}

fn first_string(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| object.get(*name))
        .find_map(value_to_string)
}

fn first_text(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| object.get(*name))
        .find_map(|value| value.as_str().map(ToOwned::to_owned))
}

fn normalize_default_value(value: Option<&Value>, is_integer: bool) -> Option<String> {
    if is_integer
        && let Some(number) = value.and_then(Value::as_f64)
        && number.fract() == 0.0
    {
        return Some((number as i64).to_string());
    }

    value.and_then(value_to_string)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn normalize_bool(value: &str) -> &str {
    match value {
        "on" | "1" => "true",
        "0" => "false",
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::catalog::ElementKind;

    #[test]
    fn normalizes_common_parameter_shapes() {
        let schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
                    {"parameterId": "size", "parameterName": "Size", "type": "BTMParameterQuantity", "defaultValue": 3},
                    {"parameterId": "color", "parameterName": "Color", "options": [{"value": "red", "label": "Red"}]},
                    {"parameterId": "enabled", "parameterName": "Enabled", "defaultValue": true}
                ]
            }),
        );

        assert_eq!(schema.parameters.len(), 3);
        assert_eq!(schema.parameters[0].kind, ParameterKind::Number);
        assert_eq!(schema.parameters[1].kind, ParameterKind::Enum);
        assert_eq!(schema.parameters[2].kind, ParameterKind::Boolean);
    }

    #[test]
    fn normalizes_unknown_parameter_shapes_as_unsupported() {
        let schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
                    {
                        "parameterId": "custom",
                        "parameterName": "Custom",
                        "type": "BTMConfigurationParameterMatrix"
                    }
                ]
            }),
        );

        assert_eq!(schema.parameters.len(), 1);
        assert_eq!(schema.parameters[0].kind, ParameterKind::Unsupported);
    }

    #[test]
    fn normalizes_missing_type_string_defaults_as_unsupported() {
        let schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
                    {
                        "parameterId": "finish",
                        "parameterName": "Finish",
                        "defaultValue": "matte"
                    }
                ]
            }),
        );

        assert_eq!(schema.parameters.len(), 1);
        assert_eq!(schema.parameters[0].kind, ParameterKind::Unsupported);
    }

    #[test]
    fn normalizes_onshape_message_wrapped_parameters() {
        let schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
                    {
                        "message": {
                            "parameterId": "xn",
                            "parameterName": "xn",
                            "quantityType": "INTEGER",
                            "rangeAndDefault": {
                                "message": {
                                    "defaultValue": 2.0,
                                    "units": ""
                                }
                            }
                        },
                        "typeName": "BTMConfigurationParameterQuantity"
                    },
                    {
                        "message": {
                            "parameterId": "wallThickness",
                            "parameterName": "wallThickness",
                            "quantityType": "LENGTH",
                            "rangeAndDefault": {
                                "message": {
                                    "defaultValue": 1.5,
                                    "units": "millimeter"
                                }
                            }
                        },
                        "typeName": "BTMConfigurationParameterQuantity"
                    },
                    {
                        "message": {
                            "defaultValue": true,
                            "parameterId": "rimColor",
                            "parameterName": "rimColor"
                        },
                        "typeName": "BTMConfigurationParameterBoolean"
                    },
                    {
                        "message": {
                            "defaultValue": "Default",
                            "options": [
                                {"message": {"option": "Default", "optionName": "None"}},
                                {"message": {"option": "Full", "optionName": "Recessed"}}
                            ],
                            "parameterId": "fillType",
                            "parameterName": "fillType"
                        },
                        "typeName": "BTMConfigurationParameterEnum"
                    },
                    {
                        "message": {
                            "defaultValue": 2,
                            "parameterId": "dividerCount",
                            "parameterName": "dividerCount",
                            "visibilityCondition": {
                                "message": {
                                    "inArray": false,
                                    "parameterId": "dividers",
                                    "value": true
                                },
                                "typeName": "BTParameterVisibilityOnEqual"
                            }
                        },
                        "typeName": "BTMConfigurationParameterQuantity"
                    }
                ]
            }),
        );

        assert_eq!(schema.parameters.len(), 5);
        assert_eq!(schema.parameters[0].id, "xn");
        assert_eq!(schema.parameters[0].default_value.as_deref(), Some("2"));
        assert_eq!(schema.parameters[0].precision, Some(0));
        assert_eq!(schema.parameters[1].id, "wallThickness");
        assert_eq!(schema.parameters[1].default_value.as_deref(), Some("1.5"));
        assert_eq!(schema.parameters[1].units.as_deref(), Some("millimeter"));
        assert_eq!(schema.parameters[1].kind, ParameterKind::Number);
        assert_eq!(schema.parameters[2].default_value.as_deref(), Some("true"));
        assert_eq!(schema.parameters[2].kind, ParameterKind::Boolean);
        assert_eq!(schema.parameters[3].kind, ParameterKind::Enum);
        assert_eq!(schema.parameters[3].options[0].label, "None");
        assert_eq!(
            schema.parameters[4].visibility_condition,
            Some(ParameterVisibilityCondition::Equal {
                parameter_id: "dividers".to_owned(),
                values: vec!["true".to_owned()],
            })
        );
    }

    #[test]
    fn normalizes_logical_visibility_conditions() {
        let schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
                    {
                        "message": {
                            "defaultValue": 1,
                            "parameterId": "partsRampRadius",
                            "parameterName": "partsRampRadius",
                            "visibilityCondition": {
                                "message": {
                                    "children": [
                                        {
                                            "message": {
                                                "inArray": false,
                                                "parameterId": "partsRamp",
                                                "value": true
                                            },
                                            "typeName": "BTParameterVisibilityOnEqual"
                                        },
                                        {
                                            "message": {
                                                "inArray": true,
                                                "parameterId": "fillType",
                                                "value": ["Default", "Full", "From_Bottom"]
                                            },
                                            "typeName": "BTParameterVisibilityOnEqual"
                                        }
                                    ],
                                    "operation": "AND"
                                },
                                "typeName": "BTParameterVisibilityLogical"
                            }
                        },
                        "typeName": "BTMConfigurationParameterQuantity"
                    }
                ]
            }),
        );

        assert_eq!(
            schema.parameters[0].visibility_condition,
            Some(ParameterVisibilityCondition::All {
                conditions: vec![
                    ParameterVisibilityCondition::Equal {
                        parameter_id: "partsRamp".to_owned(),
                        values: vec!["true".to_owned()],
                    },
                    ParameterVisibilityCondition::Equal {
                        parameter_id: "fillType".to_owned(),
                        values: vec![
                            "Default".to_owned(),
                            "Full".to_owned(),
                            "From_Bottom".to_owned(),
                        ],
                    },
                ],
            })
        );
    }

    #[test]
    fn treats_empty_and_unsupported_visibility_conditions_as_visible() {
        let schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
                    {
                        "message": {
                            "defaultValue": 1,
                            "parameterId": "alwaysVisible",
                            "parameterName": "alwaysVisible",
                            "visibilityCondition": {
                                "message": {},
                                "typeName": "BTParameterVisibilityCondition"
                            }
                        },
                        "typeName": "BTMConfigurationParameterQuantity"
                    },
                    {
                        "message": {
                            "defaultValue": 1,
                            "parameterId": "unknownVisibility",
                            "parameterName": "unknownVisibility",
                            "visibilityCondition": {
                                "message": {"other": true},
                                "typeName": "BTParameterVisibilityCustom"
                            }
                        },
                        "typeName": "BTMConfigurationParameterQuantity"
                    }
                ]
            }),
        );

        assert_eq!(schema.parameters[0].visibility_condition, None);
        assert_eq!(schema.parameters[1].visibility_condition, None);
    }

    #[test]
    fn validates_quantity_values_with_units_for_configuration() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "wallThickness".to_owned(),
                label: "Wall Thickness".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: true,
                default_value: None,
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: Some("millimeter".to_owned()),
                raw: Value::Null,
            }],
        };
        let submitted = HashMap::from([("wallThickness".to_owned(), "1.5".to_owned())]);

        let validated = validate_values(&schema, &submitted, false).unwrap();

        assert_eq!(validated.values["wallThickness"], "1.5 mm");
        assert_eq!(
            validated.typed_values["wallThickness"],
            CanonicalParameterValue::Number {
                expression: "1.5 mm".to_owned(),
                units: Some("millimeter".to_owned()),
            }
        );
    }

    #[test]
    fn preserves_user_entered_quantity_units() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "wallThickness".to_owned(),
                label: "Wall Thickness".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: true,
                default_value: Some("1.5".to_owned()),
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: Some("millimeter".to_owned()),
                raw: Value::Null,
            }],
        };
        let submitted = HashMap::from([("wallThickness".to_owned(), "0.125 in".to_owned())]);

        let validated = validate_values(&schema, &submitted, false).unwrap();

        assert_eq!(
            schema.parameters[0].display_value().as_deref(),
            Some("1.5 mm")
        );
        assert_eq!(validated.values["wallThickness"], "0.125 in");
        assert_eq!(
            validated.typed_values["wallThickness"],
            CanonicalParameterValue::Number {
                expression: "0.125 in".to_owned(),
                units: Some("millimeter".to_owned()),
            }
        );

        let compact = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "0.125in".to_owned())]),
            false,
        )
        .unwrap();
        assert_eq!(compact.values["wallThickness"], "0.125 in");
        assert_eq!(compact.typed_values, validated.typed_values);

        let long_unit = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "0.125 inch".to_owned())]),
            false,
        )
        .unwrap();
        assert_eq!(long_unit.values["wallThickness"], "0.125 in");
        assert_eq!(long_unit.typed_values, validated.typed_values);
    }

    #[test]
    fn canonicalizes_boolean_aliases_and_numeric_strings() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![
                Parameter {
                    id: "enabled".to_owned(),
                    label: "Enabled".to_owned(),
                    description: None,
                    kind: ParameterKind::Boolean,
                    required: true,
                    default_value: None,
                    options: Vec::new(),
                    hidden: false,
                    visibility_condition: None,
                    precision: None,
                    widget: None,
                    units: None,
                    raw: Value::Null,
                },
                Parameter {
                    id: "count".to_owned(),
                    label: "Count".to_owned(),
                    description: None,
                    kind: ParameterKind::Number,
                    required: true,
                    default_value: None,
                    options: Vec::new(),
                    hidden: false,
                    visibility_condition: None,
                    precision: None,
                    widget: None,
                    units: None,
                    raw: Value::Null,
                },
            ],
        };

        let first = validate_values(
            &schema,
            &HashMap::from([
                ("enabled".to_owned(), "on".to_owned()),
                ("count".to_owned(), "01.0".to_owned()),
            ]),
            false,
        )
        .unwrap();
        let second = validate_values(
            &schema,
            &HashMap::from([
                ("enabled".to_owned(), "true".to_owned()),
                ("count".to_owned(), "1".to_owned()),
            ]),
            false,
        )
        .unwrap();

        assert_eq!(first.values["enabled"], "true");
        assert_eq!(first.values["count"], "01.0");
        assert_eq!(first.typed_values, second.typed_values);
        assert_eq!(
            encoding_request_values(&first.typed_values),
            encoding_request_values(&second.typed_values)
        );
    }

    #[test]
    fn canonicalizes_encoding_request_values_for_dimensioned_numbers() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "wallThickness".to_owned(),
                label: "Wall Thickness".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: true,
                default_value: None,
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: Some("millimeter".to_owned()),
                raw: Value::Null,
            }],
        };

        let default_units = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "1.0".to_owned())]),
            false,
        )
        .unwrap();
        let explicit_units = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "01.000 mm".to_owned())]),
            false,
        )
        .unwrap();

        assert_eq!(
            encoding_request_values(&default_units.typed_values),
            encoding_request_values(&explicit_units.typed_values)
        );
        assert_eq!(
            encoding_request_values(&default_units.typed_values)["wallThickness"],
            "1 mm"
        );
    }

    #[test]
    fn rejects_invalid_unit_bearing_numbers_and_preserves_unknowns_when_allowed() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "wallThickness".to_owned(),
                label: "Wall Thickness".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: true,
                default_value: None,
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: Some("millimeter".to_owned()),
                raw: Value::Null,
            }],
        };

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "abc".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(errors, vec!["Wall Thickness must be a number"]);

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "1.5 bananas".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(errors, vec!["Wall Thickness must be a number"]);

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "foo 2 in bar".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(errors, vec!["Wall Thickness must be a number"]);

        let validated = validate_values(
            &schema,
            &HashMap::from([
                ("wallThickness".to_owned(), "1.5 mm".to_owned()),
                ("custom".to_owned(), "surprise".to_owned()),
            ]),
            true,
        )
        .unwrap();
        assert_eq!(validated.values["custom"], "surprise");
        assert_eq!(
            validated.typed_values["custom"],
            CanonicalParameterValue::Text {
                value: "surprise".to_owned(),
            }
        );
    }

    #[test]
    fn rejects_unsupported_schema_entries_even_when_unknowns_are_allowed() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "custom".to_owned(),
                label: "Custom".to_owned(),
                description: None,
                kind: ParameterKind::Unsupported,
                required: false,
                default_value: None,
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: None,
                raw: Value::Null,
            }],
        };

        let errors = validate_values(
            &schema,
            &HashMap::from([
                ("custom".to_owned(), "value".to_owned()),
                ("extra".to_owned(), "surprise".to_owned()),
            ]),
            true,
        )
        .unwrap_err();

        assert_eq!(
            errors,
            vec!["Custom (custom) uses an unsupported parameter type"]
        );
    }

    #[test]
    fn rejects_unsupported_schema_entries_without_submitted_values() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "custom".to_owned(),
                label: "Custom".to_owned(),
                description: None,
                kind: ParameterKind::Unsupported,
                required: false,
                default_value: None,
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: None,
                raw: Value::Null,
            }],
        };

        let errors = validate_values(&schema, &HashMap::new(), true).unwrap_err();

        assert_eq!(
            errors,
            vec!["Custom (custom) uses an unsupported parameter type"]
        );
    }

    #[test]
    fn applies_catalog_parameter_overrides() {
        let mut schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
                    {"parameterId": "size", "parameterName": "Size", "type": "BTMParameterQuantity", "defaultValue": 3}
                ]
            }),
        );

        apply_overrides(
            &mut schema,
            &HashMap::from([(
                "size".to_owned(),
                ParameterOverride {
                    label: Some("Public Size".to_owned()),
                    description: Some("Shown to users".to_owned()),
                    hidden: true,
                    precision: Some(2),
                    widget: Some("slider".to_owned()),
                },
            )]),
        );

        assert_eq!(schema.parameters[0].label, "Public Size");
        assert_eq!(
            schema.parameters[0].description.as_deref(),
            Some("Shown to users")
        );
        assert!(schema.parameters[0].hidden);
        assert_eq!(schema.parameters[0].precision, Some(2));
        assert_eq!(schema.parameters[0].widget.as_deref(), Some("slider"));
    }

    #[test]
    fn rejects_unknown_and_invalid_values() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "size".to_owned(),
                label: "Size".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: true,
                default_value: None,
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: None,
                raw: Value::Null,
            }],
        };
        let submitted = HashMap::from([
            ("size".to_owned(), "large".to_owned()),
            ("extra".to_owned(), "x".to_owned()),
        ]);

        let errors = validate_values(&schema, &submitted, false).unwrap_err();

        assert_eq!(errors.len(), 2);
    }

    fn source() -> OnshapeSource {
        OnshapeSource {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        }
    }
}
