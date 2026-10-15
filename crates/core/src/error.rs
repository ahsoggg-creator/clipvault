use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),
    #[error("хранилище повреждено или ключ не подходит")]
    Corrupt,
    #[error("ключ шифрования недоступен: {0}")]
    Key(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn other(msg: impl fmt::Display) -> Error {
        Error::Other(msg.to_string())
    }
}
