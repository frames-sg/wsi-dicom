use wsi_dicom::Error;

pub(crate) fn cli_output_line<T, F>(json: bool, value: &T, summary: F) -> Result<String, Error>
where
    T: serde::Serialize,
    F: FnOnce(&T) -> String,
{
    if json {
        serde_json::to_string(value).map_err(|source| Error::JsonSerialize {
            message: source.to_string(),
        })
    } else {
        Ok(summary(value))
    }
}

pub(crate) fn print_cli_output<T, F>(json: bool, value: &T, summary: F) -> Result<(), Error>
where
    T: serde::Serialize,
    F: FnOnce(&T) -> String,
{
    println!("{}", cli_output_line(json, value, summary)?);
    Ok(())
}

pub(crate) fn print_json_line<T: serde::Serialize>(value: &T) -> Result<(), Error> {
    let json = serde_json::to_string(value).map_err(|source| Error::JsonSerialize {
        message: source.to_string(),
    })?;
    println!("{json}");
    Ok(())
}
