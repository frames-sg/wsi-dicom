//! Tests of the DICOMweb checks. The slide fixtures are shared with the DICOM
//! JSON tests; the checks against an in-process origin server need the
//! `dicomweb` transport.

pub(crate) mod fixture;
#[cfg(feature = "dicomweb")]
mod mock;
#[cfg(feature = "dicomweb")]
mod server;
