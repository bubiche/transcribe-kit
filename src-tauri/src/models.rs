use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelDescriptor {
    pub id: String,
    pub label: String,
    pub engine: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiModelDescriptor {
    pub id: String,
    pub label: String,
    pub provider: String,
}

