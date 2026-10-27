use std::{fmt, str::FromStr};

pub enum QMLBasicType {
    Int,
    Real,
    Bool,
    String,
    Var,
}

pub enum QMLPropertyType {
    BuiltIn(QMLBasicType),
    Custom(String),
}

pub struct QMLProperty {
    pub(crate) name: String,
    pub(crate) p_type: QMLPropertyType,
    pub(crate) description: Option<String>,
}

pub struct QMLSignal {
    name: String,
    parameters: Vec<QMLParameter>,
}
pub struct QMLMethod {
    name: String,
    parameters: Vec<QMLParameter>,
}
pub struct QMLParameter {
    name: String,
    p_type: QMLBasicType,
}

pub struct QMLComponent {
    pub cpp_name: String,
    pub qml_name: Option<String>,
    pub(crate) prototype: Option<String>,
    pub(crate) module: Option<String>,

    pub(crate) properties: Vec<QMLProperty>,
    pub(crate) signals: Vec<QMLSignal>,
    pub(crate) methods: Vec<QMLMethod>,
}

impl FromStr for QMLBasicType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "int" => Ok(Self::Int),
            "real" => Ok(Self::Real),
            "bool" => Ok(Self::Bool),
            "string" => Ok(Self::String),
            "var" => Ok(Self::Var),
            _ => Err(()),
        }
    }
}

impl fmt::Display for QMLBasicType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let basic_type_str = match self {
            QMLBasicType::Int => "int",
            QMLBasicType::Real => "real",
            QMLBasicType::Bool => "bool",
            QMLBasicType::String => "string",
            QMLBasicType::Var => "var",
        };
        write!(f, "{}", basic_type_str)
    }
}

impl From<String> for QMLPropertyType {
    fn from(value: String) -> Self {
        match QMLBasicType::from_str(&value) {
            Ok(basic_type) => Self::BuiltIn(basic_type),
            Err(_) => Self::Custom(value),
        }
    }
}

impl fmt::Display for QMLPropertyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QMLPropertyType::BuiltIn(basic_type) => write!(f, "{}", basic_type),
            QMLPropertyType::Custom(custom) => write!(f, "{}", custom),
        }
    }
}

impl QMLComponent {
    pub fn name(&self) -> &str {
        match &self.qml_name {
            Some(name) => name,
            None => &self.cpp_name,
        }
    }
}
