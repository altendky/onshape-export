use std::collections::{BTreeMap, HashMap, HashSet};

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::{OnshapeSource, ParameterOverride};

pub const SCHEMA_VERSION: u32 = 4;

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
        numerator: String,
        denominator: String,
    },
    Quantity {
        dimension: QuantityDimension,
        numerator: String,
        denominator: String,
        unit: String,
    },
    Boolean {
        value: bool,
    },
    Enum {
        value: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityDimension {
    Length,
    Angle,
}

#[derive(Debug, Clone, Copy)]
pub struct QuantityUnitOption {
    pub value: &'static str,
    pub label: &'static str,
}

const LENGTH_UNIT_OPTIONS: &[QuantityUnitOption] = &[
    QuantityUnitOption {
        value: "mm",
        label: "mm",
    },
    QuantityUnitOption {
        value: "cm",
        label: "cm",
    },
    QuantityUnitOption {
        value: "m",
        label: "m",
    },
    QuantityUnitOption {
        value: "in",
        label: "in",
    },
    QuantityUnitOption {
        value: "ft",
        label: "ft",
    },
];

const ANGLE_UNIT_OPTIONS: &[QuantityUnitOption] = &[
    QuantityUnitOption {
        value: "deg",
        label: "deg",
    },
    QuantityUnitOption {
        value: "rad",
        label: "rad",
    },
];

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
                CanonicalParameterValue::Number {
                    numerator,
                    denominator,
                } => fraction_expression(numerator, denominator),
                CanonicalParameterValue::Quantity {
                    numerator,
                    denominator,
                    unit,
                    ..
                } => format!("{} {unit}", fraction_expression(numerator, denominator)),
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
                        canonicalize_unitless_number(parameter, value)
                    };
                    match canonical {
                        Ok((canonical, value)) => {
                            values.insert(parameter.id.clone(), value);
                            typed_values.insert(parameter.id.clone(), canonical);
                        }
                        Err(error) => errors.push(error),
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
            .is_some_and(|value| parse_decimal_rational(value).is_some())
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
        let Some(number) = parse_decimal_rational(value) else {
            return value.to_owned();
        };
        let number = rational_decimal_string(&number).expect("decimal input terminates");

        match self
            .units
            .as_deref()
            .and_then(QuantityUnit::from_onshape_unit)
        {
            Some(unit) => format!("{number} {}", unit.abbreviation()),
            None => number,
        }
    }
}

pub fn quantity_unit_options(parameter: &Parameter) -> Option<&'static [QuantityUnitOption]> {
    let unit = parameter
        .units
        .as_deref()
        .and_then(QuantityUnit::from_onshape_unit)?;
    match unit.dimension() {
        QuantityDimension::Length => Some(LENGTH_UNIT_OPTIONS),
        QuantityDimension::Angle => Some(ANGLE_UNIT_OPTIONS),
    }
}

pub fn default_quantity_unit(parameter: &Parameter) -> Option<&'static str> {
    parameter
        .units
        .as_deref()
        .and_then(QuantityUnit::from_onshape_unit)
        .map(QuantityUnit::abbreviation)
}

fn canonicalize_dimensioned_number(
    parameter: &Parameter,
    value: &str,
) -> Result<(CanonicalParameterValue, String), String> {
    let default_unit = parameter
        .units
        .as_deref()
        .and_then(QuantityUnit::from_onshape_unit)
        .ok_or_else(|| format!("{} uses an unsupported unit", parameter.label))?;
    let (number, unit) = parse_quantity(value, default_unit)
        .map_err(|error| quantity_error_message(parameter, error, default_unit.dimension()))?;
    let canonical = canonical_quantity_value(number, unit);
    let normalized = format!(
        "{} {}",
        rational_decimal_string(&canonical.value).expect("canonical value terminates"),
        canonical.unit.abbreviation()
    );
    let (numerator, denominator) = rational_parts(&canonical.value);

    Ok((
        CanonicalParameterValue::Quantity {
            dimension: canonical.unit.dimension(),
            numerator,
            denominator,
            unit: canonical.unit.abbreviation().to_owned(),
        },
        normalized,
    ))
}

fn canonicalize_unitless_number(
    parameter: &Parameter,
    value: &str,
) -> Result<(CanonicalParameterValue, String), String> {
    let number = parse_decimal_rational(value)
        .ok_or_else(|| format!("{} must be a plain decimal number", parameter.label))?;
    let normalized = rational_decimal_string(&number).expect("decimal input terminates");
    let (numerator, denominator) = rational_parts(&number);
    Ok((
        CanonicalParameterValue::Number {
            numerator,
            denominator,
        },
        normalized,
    ))
}

fn unsupported_parameter_message(parameter: &Parameter) -> String {
    format!(
        "{} ({}) uses an unsupported parameter type",
        parameter.label, parameter.id
    )
}

fn fraction_expression(numerator: &str, denominator: &str) -> String {
    format!("({numerator}/{denominator})")
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum QuantityUnit {
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Foot,
    Degree,
    Radian,
}

impl QuantityUnit {
    fn from_onshape_unit(unit: &str) -> Option<Self> {
        match unit {
            "millimeter" => Some(Self::Millimeter),
            "centimeter" => Some(Self::Centimeter),
            "meter" => Some(Self::Meter),
            "inch" => Some(Self::Inch),
            "foot" => Some(Self::Foot),
            "degree" => Some(Self::Degree),
            "radian" => Some(Self::Radian),
            _ => None,
        }
    }

    fn from_abbreviation(unit: &str) -> Option<Self> {
        match unit {
            "mm" => Some(Self::Millimeter),
            "cm" => Some(Self::Centimeter),
            "m" => Some(Self::Meter),
            "in" => Some(Self::Inch),
            "ft" => Some(Self::Foot),
            "deg" => Some(Self::Degree),
            "rad" => Some(Self::Radian),
            _ => None,
        }
    }

    fn abbreviation(self) -> &'static str {
        match self {
            Self::Millimeter => "mm",
            Self::Centimeter => "cm",
            Self::Meter => "m",
            Self::Inch => "in",
            Self::Foot => "ft",
            Self::Degree => "deg",
            Self::Radian => "rad",
        }
    }

    fn dimension(self) -> QuantityDimension {
        match self {
            Self::Millimeter | Self::Centimeter | Self::Meter | Self::Inch | Self::Foot => {
                QuantityDimension::Length
            }
            Self::Degree | Self::Radian => QuantityDimension::Angle,
        }
    }

    fn length_meters_factor(self) -> Option<BigRational> {
        match self {
            Self::Millimeter => Some(rational(1, 1000)),
            Self::Centimeter => Some(rational(1, 100)),
            Self::Meter => Some(rational(1, 1)),
            Self::Inch => Some(rational(127, 5000)),
            Self::Foot => Some(rational(381, 1250)),
            Self::Degree | Self::Radian => None,
        }
    }
}

struct CanonicalQuantity {
    value: BigRational,
    unit: QuantityUnit,
}

enum QuantityParseError {
    Number,
    UnknownUnit,
    IncompatibleUnit,
}

fn parse_quantity(
    value: &str,
    default_unit: QuantityUnit,
) -> Result<(BigRational, QuantityUnit), QuantityParseError> {
    let trimmed = value.trim();
    let (number, unit) = match trailing_quantity_unit(trimmed) {
        Some((unit_start, unit)) => {
            if unit_start == 0 {
                return Err(QuantityParseError::Number);
            }
            let unit =
                QuantityUnit::from_abbreviation(unit).ok_or(QuantityParseError::UnknownUnit)?;
            (trimmed[..unit_start].trim(), unit)
        }
        None => (trimmed, default_unit),
    };
    if unit.dimension() != default_unit.dimension() {
        return Err(QuantityParseError::IncompatibleUnit);
    }
    let number = parse_decimal_rational(number).ok_or(QuantityParseError::Number)?;
    Ok((number, unit))
}

fn canonical_quantity_value(value: BigRational, unit: QuantityUnit) -> CanonicalQuantity {
    match unit.dimension() {
        QuantityDimension::Length => CanonicalQuantity {
            value: value * unit.length_meters_factor().expect("length unit has factor"),
            unit: QuantityUnit::Meter,
        },
        QuantityDimension::Angle => CanonicalQuantity { value, unit },
    }
}

fn quantity_error_message(
    parameter: &Parameter,
    error: QuantityParseError,
    dimension: QuantityDimension,
) -> String {
    match error {
        QuantityParseError::Number => {
            format!("{} must be a plain decimal number", parameter.label)
        }
        QuantityParseError::UnknownUnit => format!("{} has an unknown unit", parameter.label),
        QuantityParseError::IncompatibleUnit => match dimension {
            QuantityDimension::Length => format!("{} must use a length unit", parameter.label),
            QuantityDimension::Angle => format!("{} must use an angle unit", parameter.label),
        },
    }
}

fn parse_decimal_rational(value: &str) -> Option<BigRational> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (negative, unsigned) = match trimmed.as_bytes()[0] {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if unsigned.is_empty() {
        return None;
    }

    let mut pieces = unsigned.split('.');
    let integer = pieces.next()?;
    let fractional = pieces.next();
    if pieces.next().is_some() {
        return None;
    }
    let fractional = fractional.unwrap_or("");
    if integer.is_empty() && fractional.is_empty() {
        return None;
    }
    if !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fractional.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }

    let digits = format!("{integer}{fractional}");
    let mut numerator = BigInt::parse_bytes(digits.as_bytes(), 10)?;
    if negative {
        numerator = -numerator;
    }
    let denominator = BigInt::from(10_u32).pow(fractional.len() as u32);
    Some(BigRational::new(numerator, denominator))
}

fn rational(numerator: i64, denominator: i64) -> BigRational {
    BigRational::new(BigInt::from(numerator), BigInt::from(denominator))
}

fn rational_parts(value: &BigRational) -> (String, String) {
    (value.numer().to_string(), value.denom().to_string())
}

fn rational_decimal_string(value: &BigRational) -> Option<String> {
    let mut denominator = value.denom().clone();
    let two = BigInt::from(2);
    let five = BigInt::from(5);
    while (&denominator % &two).is_zero() {
        denominator /= &two;
    }
    while (&denominator % &five).is_zero() {
        denominator /= &five;
    }
    if !denominator.is_one() {
        return None;
    }

    let sign = if value.numer().is_negative() { "-" } else { "" };
    let numerator = value.numer().abs();
    let denominator = value.denom();
    let integer = &numerator / denominator;
    let mut remainder = &numerator % denominator;
    if remainder.is_zero() {
        if integer.is_zero() {
            return Some("0".to_owned());
        }
        return Some(format!("{sign}{integer}"));
    }

    let ten = BigInt::from(10);
    let mut fractional = String::new();
    while !remainder.is_zero() {
        remainder *= &ten;
        let digit = &remainder / denominator;
        fractional.push_str(&digit.to_string());
        remainder %= denominator;
    }
    while fractional.ends_with('0') {
        fractional.pop();
    }

    if integer.is_zero() && sign == "-" && fractional.bytes().all(|byte| byte == b'0') {
        Some("0".to_owned())
    } else {
        Some(format!("{sign}{integer}.{fractional}"))
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
    let value = value.and_then(value_to_string)?;
    if is_integer
        && let Some(number) = parse_decimal_rational(&value)
        && number.denom().is_one()
    {
        return Some(number.numer().to_string());
    }

    Some(value)
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

        assert_eq!(validated.values["wallThickness"], "0.0015 m");
        assert_eq!(
            validated.typed_values["wallThickness"],
            CanonicalParameterValue::Quantity {
                dimension: QuantityDimension::Length,
                numerator: "3".to_owned(),
                denominator: "2000".to_owned(),
                unit: "m".to_owned(),
            }
        );
    }

    #[test]
    fn canonicalizes_length_quantities_to_meters() {
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
        assert_eq!(validated.values["wallThickness"], "0.003175 m");
        assert_eq!(
            validated.typed_values["wallThickness"],
            CanonicalParameterValue::Quantity {
                dimension: QuantityDimension::Length,
                numerator: "127".to_owned(),
                denominator: "40000".to_owned(),
                unit: "m".to_owned(),
            }
        );

        let compact = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "0.125in".to_owned())]),
            false,
        )
        .unwrap();
        assert_eq!(compact.values["wallThickness"], "0.003175 m");
        assert_eq!(compact.typed_values, validated.typed_values);

        let meters = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "0.003175 m".to_owned())]),
            false,
        )
        .unwrap();
        assert_eq!(meters.values["wallThickness"], "0.003175 m");
        assert_eq!(meters.typed_values, validated.typed_values);

        let millimeters = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "3.175 mm".to_owned())]),
            false,
        )
        .unwrap();
        assert_eq!(millimeters.typed_values, validated.typed_values);
        assert_eq!(
            encoding_request_values(&validated.typed_values)["wallThickness"],
            "(127/40000) m"
        );
    }

    #[test]
    fn equivalent_length_values_share_typed_values_and_config_hash() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "length".to_owned(),
                label: "Length".to_owned(),
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

        let meter = validate_values(
            &schema,
            &HashMap::from([("length".to_owned(), "1m".to_owned())]),
            false,
        )
        .unwrap();
        let millimeter = validate_values(
            &schema,
            &HashMap::from([("length".to_owned(), "1000 mm".to_owned())]),
            false,
        )
        .unwrap();

        assert_eq!(meter.typed_values, millimeter.typed_values);
        assert_eq!(
            crate::cache_model::config_hash("source", SCHEMA_VERSION, &meter.typed_values).unwrap(),
            crate::cache_model::config_hash("source", SCHEMA_VERSION, &millimeter.typed_values)
                .unwrap()
        );
        assert_eq!(
            encoding_request_values(&meter.typed_values)["length"],
            "(1/1) m"
        );
    }

    #[test]
    fn canonicalizes_angle_values_without_cross_unit_conversion() {
        let schema = ParameterSchema {
            schema_version: SCHEMA_VERSION,
            source: source(),
            parameters: vec![Parameter {
                id: "angle".to_owned(),
                label: "Angle".to_owned(),
                description: None,
                kind: ParameterKind::Number,
                required: true,
                default_value: None,
                options: Vec::new(),
                hidden: false,
                visibility_condition: None,
                precision: None,
                widget: None,
                units: Some("degree".to_owned()),
                raw: Value::Null,
            }],
        };

        let degrees = validate_values(
            &schema,
            &HashMap::from([("angle".to_owned(), "180".to_owned())]),
            false,
        )
        .unwrap();
        let radians = validate_values(
            &schema,
            &HashMap::from([("angle".to_owned(), "3.14159 rad".to_owned())]),
            false,
        )
        .unwrap();

        assert_eq!(degrees.values["angle"], "180 deg");
        assert_eq!(radians.values["angle"], "3.14159 rad");
        assert_ne!(degrees.typed_values, radians.typed_values);
        assert_eq!(
            encoding_request_values(&degrees.typed_values)["angle"],
            "(180/1) deg"
        );
        assert_eq!(
            encoding_request_values(&radians.typed_values)["angle"],
            "(314159/100000) rad"
        );
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
        assert_eq!(first.values["count"], "1");
        assert_eq!(first.typed_values, second.typed_values);
        assert_eq!(
            encoding_request_values(&first.typed_values),
            encoding_request_values(&second.typed_values)
        );
        assert_eq!(
            encoding_request_values(&first.typed_values)["count"],
            "(1/1)"
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
            "(1/1000) m"
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
        assert_eq!(
            errors,
            vec!["Wall Thickness must be a plain decimal number"]
        );

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "1.5 bananas".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(errors, vec!["Wall Thickness has an unknown unit"]);

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "1.5 deg".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(errors, vec!["Wall Thickness must use a length unit"]);

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "1e-3 mm".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(
            errors,
            vec!["Wall Thickness must be a plain decimal number"]
        );

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "1 MM".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(errors, vec!["Wall Thickness has an unknown unit"]);

        let errors = validate_values(
            &schema,
            &HashMap::from([("wallThickness".to_owned(), "NaN mm".to_owned())]),
            false,
        )
        .unwrap_err();
        assert_eq!(
            errors,
            vec!["Wall Thickness must be a plain decimal number"]
        );

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
