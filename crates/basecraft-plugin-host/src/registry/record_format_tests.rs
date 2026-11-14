//! Current index behavior; protocol's central registry checks the wire goldens.
use super::*;
use crate::error::protocol;

fn config(root: &Path) -> RegistryConfig {
    RegistryConfig {
        bundled: root.join("bundled"),
        packages: root.join("packages"),
        index: root.join("config/plugins-v1.json"),
    }
}

#[test]
fn index_requires_current_fields_and_enforces_read_limit() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    assert_eq!(load_index(&config).unwrap().schema, 1);
    assert!(!config.index.parent().unwrap().exists());
    write_index(&config, |_| Ok(())).unwrap();
    let mut value = serde_json::to_value(load_index(&config).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("schema");
    std::fs::write(&config.index, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(load_index(&config).is_err());
    value["schema"] = 0.into();
    std::fs::write(&config.index, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(load_index(&config).is_err());
    std::fs::File::create(&config.index)
        .unwrap()
        .set_len(MAX_DESCRIPTOR_BYTES as u64 + 1)
        .unwrap();
    assert!(
        load_index(&config)
            .unwrap_err()
            .to_string()
            .contains("exceeds size limit")
    );
}

#[test]
fn index_mutation_failures_preserve_previous_file() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    write_index(&config, |index| {
        index.active.insert("plugin".into(), "digest".into());
        index.disabled.insert("plugin".into());
        Ok(())
    })
    .unwrap();
    write_index(&config, |index| {
        index.disabled.remove("plugin");
        Ok(())
    })
    .unwrap();
    assert_eq!(load_index(&config).unwrap().active["plugin"], "digest");
    assert!(load_index(&config).unwrap().disabled.is_empty());
    let original = std::fs::read(&config.index).unwrap();
    assert!(write_index(&config, |_| Err(protocol("refused"))).is_err());
    assert_eq!(std::fs::read(&config.index).unwrap(), original);
    assert!(
        write_index(&config, |index| {
            index
                .active
                .insert("large".into(), "x".repeat(MAX_DESCRIPTOR_BYTES));
            Ok(())
        })
        .is_err()
    );
    assert_eq!(std::fs::read(&config.index).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        assert!(
            write_index(&config, |index| {
                index
                    .editable
                    .insert(PathBuf::from(std::ffi::OsString::from_vec(vec![
                        b'x', 0xff,
                    ])));
                Ok(())
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&config.index).unwrap(), original);
    }
    std::fs::write(&config.index, b"broken").unwrap();
    assert!(write_index(&config, |_| Ok(())).is_err());
    assert_eq!(std::fs::read(&config.index).unwrap(), b"broken");
}
