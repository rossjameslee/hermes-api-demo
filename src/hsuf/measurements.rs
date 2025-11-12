use crate::hsuf::models::QuantitativeValue;

const LENGTH_CODE_TO_INCHES: &[(&str, f64)] = &[
    ("INH", 1.0),
    ("FT", 12.0),
    ("CMT", 0.3937007874),
    ("MTR", 39.37007874),
    ("MMT", 0.03937007874),
    ("YRD", 36.0),
];

const WEIGHT_CODE_TO_POUNDS: &[(&str, f64)] = &[
    ("LBR", 1.0),
    ("ONZ", 0.0625),
    ("KGM", 2.20462262),
    ("GRM", 0.00220462262),
];

pub fn quantitative_length_to_inches(value: &Option<QuantitativeValue>) -> Option<f64> {
    let value = value.as_ref()?;
    let number = value.value?;
    if number <= 0.0 {
        return None;
    }
    let code = normalize_length_code(value.unitCode.as_deref(), value.unitText.as_deref())?;
    let factor = LENGTH_CODE_TO_INCHES
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(&code))?
        .1;
    Some(number * factor)
}

pub fn quantitative_weight_to_pounds(value: &Option<QuantitativeValue>) -> Option<f64> {
    let value = value.as_ref()?;
    let number = value.value?;
    if number <= 0.0 {
        return None;
    }
    let code = normalize_weight_code(value.unitCode.as_deref(), value.unitText.as_deref())?;
    let factor = WEIGHT_CODE_TO_POUNDS
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(&code))?
        .1;
    Some(number * factor)
}

fn normalize_length_code(code: Option<&str>, text: Option<&str>) -> Option<String> {
    if let Some(code) = code {
        let upper = code.trim().to_uppercase();
        if LENGTH_CODE_TO_INCHES.iter().any(|(key, _)| key == &upper) {
            return Some(upper);
        }
    }
    match text.map(|value| value.trim().to_lowercase()).as_deref() {
        Some("inch") | Some("inches") | Some("in") => Some("INH".into()),
        Some("foot") | Some("feet") | Some("ft") => Some("FT".into()),
        Some("centimeter") | Some("centimeters") | Some("cm") => Some("CMT".into()),
        Some("meter") | Some("meters") | Some("m") => Some("MTR".into()),
        Some("millimeter") | Some("millimeters") | Some("mm") => Some("MMT".into()),
        Some("yard") | Some("yards") => Some("YRD".into()),
        _ => None,
    }
}

fn normalize_weight_code(code: Option<&str>, text: Option<&str>) -> Option<String> {
    if let Some(code) = code {
        let upper = code.trim().to_uppercase();
        if WEIGHT_CODE_TO_POUNDS.iter().any(|(key, _)| key == &upper) {
            return Some(upper);
        }
    }
    match text.map(|value| value.trim().to_lowercase()).as_deref() {
        Some("pound") | Some("pounds") | Some("lb") | Some("lbs") => Some("LBR".into()),
        Some("ounce") | Some("ounces") | Some("oz") => Some("ONZ".into()),
        Some("kilogram") | Some("kilograms") | Some("kg") => Some("KGM".into()),
        Some("gram") | Some("grams") | Some("g") => Some("GRM".into()),
        _ => None,
    }
}

pub fn round_one(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

pub fn round_two(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
