pub struct QMLProperty {
    pub(crate) name: String,
    pub(crate) type_name: String,
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
    // temporary string, this has to be a primitive type (int, bool, etc.) or a component
    p_type: String,
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

impl QMLComponent {
    pub fn name(&self) -> &str {
        match &self.qml_name {
            Some(name) => name,
            None => &self.cpp_name,
        }
    }
}
