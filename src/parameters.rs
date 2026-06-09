use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::{OnshapeSource, ParameterOverride};

pub const SCHEMA_VERSION: u32 = 1;

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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValidatedConfiguration {
    pub values: HashMap<String, String>,
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
    for parameter in &schema.parameters {
        let value = submitted
            .get(&parameter.id)
            .filter(|value| !value.is_empty())
            .or(parameter.default_value.as_ref());

        match value {
            Some(value) => match parameter.kind {
                ParameterKind::Text => {
                    values.insert(parameter.id.clone(), value.clone());
                }
                ParameterKind::Number => {
                    let valid = if parameter.units.is_some() {
                        !value.trim().is_empty()
                    } else {
                        value.parse::<f64>().is_ok()
                    };
                    if valid {
                        values.insert(parameter.id.clone(), parameter.configuration_value(value));
                    } else {
                        errors.push(format!("{} must be a number", parameter.label));
                    }
                }
                ParameterKind::Boolean => match value.as_str() {
                    "true" | "false" | "on" | "0" | "1" => {
                        values.insert(parameter.id.clone(), normalize_bool(value).to_owned());
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
                    } else {
                        errors.push(format!("{} has an invalid option", parameter.label));
                    }
                }
            },
            None if parameter.required => errors.push(format!("{} is required", parameter.label)),
            None => {}
        }
    }

    if errors.is_empty() {
        Ok(ValidatedConfiguration { values })
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
    let options = extract_options(value);
    let default_value = message
        .get("defaultValue")
        .or_else(|| message.get("default"))
        .or_else(|| message.get("value"))
        .or_else(|| range_and_default_message(message).and_then(|value| value.get("defaultValue")))
        .and_then(value_to_string);
    let units = range_and_default_message(message)
        .and_then(|value| value.get("units"))
        .and_then(value_to_string)
        .filter(|units| !units.is_empty());
    let type_hint = first_text(object, &["typeName", "parameterType", "type"])
        .or_else(|| {
            first_text(
                message,
                &["typeName", "parameterType", "quantityType", "type"],
            )
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
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
    } else {
        ParameterKind::Text
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
        precision: None,
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
    fn normalizes_onshape_message_wrapped_parameters() {
        let schema = normalize_configuration(
            &source(),
            &json!({
                "configurationParameters": [
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
                    }
                ]
            }),
        );

        assert_eq!(schema.parameters.len(), 3);
        assert_eq!(schema.parameters[0].id, "wallThickness");
        assert_eq!(schema.parameters[0].default_value.as_deref(), Some("1.5"));
        assert_eq!(schema.parameters[0].units.as_deref(), Some("millimeter"));
        assert_eq!(schema.parameters[0].kind, ParameterKind::Number);
        assert_eq!(schema.parameters[1].default_value.as_deref(), Some("true"));
        assert_eq!(schema.parameters[1].kind, ParameterKind::Boolean);
        assert_eq!(schema.parameters[2].kind, ParameterKind::Enum);
        assert_eq!(schema.parameters[2].options[0].label, "None");
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
                precision: None,
                widget: None,
                units: Some("millimeter".to_owned()),
                raw: Value::Null,
            }],
        };
        let submitted = HashMap::from([("wallThickness".to_owned(), "1.5".to_owned())]);

        let validated = validate_values(&schema, &submitted, false).unwrap();

        assert_eq!(validated.values["wallThickness"], "1.5 mm");
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
        }
    }
}
