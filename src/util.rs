use crate::error::Error;
use chrono::prelude::*;
use kube::runtime::finalizer::Error as FinalizerError;

pub static NS: &str = "default";

pub fn map_finalizer_error(e: FinalizerError<Error>) -> Error {
    match e {
        FinalizerError::AddFinalizer(error) => error.into(),
        FinalizerError::RemoveFinalizer(error) => error.into(),
        FinalizerError::ApplyFailed(error) => error,
        FinalizerError::CleanupFailed(error) => error,
        FinalizerError::UnnamedObject => {
            Error::InvalidKubernetesObject("Object has no name".to_string())
        }
    }
}

pub fn _now() -> DateTime<Utc> {
    Utc::now()
}

pub fn timestamp_now() -> String {
    let utc: DateTime<Utc> = Utc::now();
    utc.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn _parse_timestamp(input: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(input).ok().map(|dt| dt.into())
}
