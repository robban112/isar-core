use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub enum DataType {
    #[serde(alias = "Bool")]
    Byte,
    Int,
    Float,
    Long,
    Double,
    String,
    #[serde(alias = "BoolList")]
    ByteList,
    IntList,
    FloatList,
    LongList,
    DoubleList,
    StringList,
}

impl DataType {
    pub fn is_static(&self) -> bool {
        matches!(
            &self,
            DataType::Int | DataType::Long | DataType::Float | DataType::Double | DataType::Byte
        )
    }

    pub fn is_dynamic(&self) -> bool {
        !self.is_static()
    }

    pub fn get_static_size(&self) -> usize {
        match *self {
            DataType::Byte => 1,
            DataType::Int | DataType::Float => 4,
            _ => 8,
        }
    }

    pub fn is_scalar(&self) -> bool {
        self.get_element_type().is_none()
    }

    pub fn get_element_type(&self) -> Option<DataType> {
        match self {
            DataType::ByteList => Some(DataType::Byte),
            DataType::IntList => Some(DataType::Int),
            DataType::FloatList => Some(DataType::Float),
            DataType::LongList => Some(DataType::Long),
            DataType::DoubleList => Some(DataType::Double),
            DataType::StringList => Some(DataType::String),
            _ => None,
        }
    }
}
