use crate::Error;

/// Serializes a value into a byte buffer
pub fn serialize<V: serde::Serialize>(value: V) -> Result<Vec<u8>, Error> {
    let mut buffer = Vec::new();
    value.serialize(&mut rmps::Serializer::new(&mut buffer))?;
    Ok(buffer)
}

/// Deserializes a value from a byte buffer
pub fn deserialize<'de, V: serde::Deserialize<'de>>(bytes: Vec<u8>) -> Result<V, Error> {
    let mut deserializer = rmps::Deserializer::new(&bytes[..]);
    Ok(serde::Deserialize::deserialize(&mut deserializer)?)
}
