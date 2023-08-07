use std::fmt;

#[derive(Debug)]
pub struct EngineError {
    details: String,
}

impl EngineError {
    pub fn new(msg: &str) -> EngineError {
        EngineError{ details: msg.to_string() }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl std::error::Error for EngineError {
    fn description(&self) -> &str {
        &self.details
    }
}

impl From<specs::error::Error> for EngineError {
    fn from(error: specs::error::Error) -> Self {
        EngineError::new(&format!("Specs error: {}", error))
    }
}