//! Tests of the DICOMweb checks. The slide fixtures are shared with the DICOM
//! JSON tests; the checks against an in-process origin server need the
//! `dicomweb` transport.

#[cfg(feature = "dicomweb")]
mod breaks;
#[cfg(feature = "dicomweb")]
mod conventions;
pub(crate) mod fixture;
#[cfg(feature = "dicomweb")]
pub(super) mod mock;
#[cfg(feature = "dicomweb")]
mod origin;
#[cfg(feature = "dicomweb")]
mod rendering;
#[cfg(feature = "dicomweb")]
mod server;
