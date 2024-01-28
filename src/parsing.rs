use anyhow::{Context, Result};
use std::{
    convert::identity,
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_xml_rs::from_reader;

use crate::filtering::Package;

#[derive(Debug, Deserialize)]
pub struct PackageXml {
    pub name: String,
    #[serde(default)]
    pub depend: Vec<String>,
    #[serde(default)]
    pub build_depend: Vec<String>,
    #[serde(default)]
    pub test_depend: Vec<String>,
    #[serde(default)]
    pub exec_depend: Vec<String>,
}

pub struct Entry {
    pub pkg: PackageXml,
    pub path: PathBuf,
}

enum SearchOutcome {
    Found(Entry),
    Ignored,
    IsFile,
    Recurse,
}

static IGNORE_MARKERS: [&str; 3] = ["COLCON_IGNORE", "CATKIN_IGNORE", "AMENT_IGNORE"];

// TODO: follow symlinks?
fn check_path(dir: &Path) -> Result<SearchOutcome> {
    use SearchOutcome::*;
    if !dir.is_dir() {
        return Ok(SearchOutcome::IsFile {});
    }

    let is_dot_file = dir
        .file_name()
        .map(|x| x.to_string_lossy())
        .map(|x| x.starts_with('.'));
    if let Some(true) = is_dot_file {
        return Ok(Ignored {});
    }

    if IGNORE_MARKERS
        .iter()
        .any(|ignore| dir.join(ignore).try_exists().is_ok_and(identity))
    {
        return Ok(Ignored {});
    }

    let pkg_xml = dir.join("package.xml");
    if pkg_xml
        .try_exists()
        .with_context(|| format!("Wile trying to check '{}'", pkg_xml.display()))?
    {
        return parse_package(&pkg_xml).map(|pkg| {
            Found(Entry {
                pkg,
                path: dir.to_path_buf(),
            })
        });
    }
    Ok(Recurse {})
}

fn parse_package(xml_file: &PathBuf) -> Result<PackageXml> {
    let context = || format!("While trying to parse '{}'", xml_file.display());
    let f = File::open(xml_file).with_context(context)?;
    let reader = BufReader::new(f);
    from_reader(reader).with_context(context)
}

fn find_packages(dir: &Path, results: &mut Vec<Package>, recurse: bool) -> anyhow::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    use SearchOutcome::*;
    for entry in (fs::read_dir(dir)
        .with_context(|| format!("While searching '{}'", dir.display()))?)
    .flatten()
    {
        let check_outcome = check_path(&entry.path())?;
        match check_outcome {
            Found(entry) => {
                results.push(entry.into());
            }
            Recurse if recurse => {
                find_packages(&entry.path(), results, recurse)?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn find(dir: &Path) -> anyhow::Result<Vec<Package>> {
    let mut res: Vec<_> = Vec::new();
    if let SearchOutcome::Found(entry) = check_path(dir)? {
        res.push(entry.into());
    }
    find_packages(dir, &mut res, true)?;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use crate::parsing::*;
    use serde_xml_rs::from_str;

    #[test]
    fn fails_on_broken() {
        {
            let manifest1 = r#"<?xml version="1.0"?>
            <?xml-model href="http://download.ros.org/schema/package_format3.xsd" schematypens="http://www.w3.org/2001/XMLSchema"?>
            farts
            </package>
            "#;
            from_str::<PackageXml>(manifest1).expect_err("Should not have parsed!");
        }
        {
            let manifest2 = r#"nothing"#;
            from_str::<PackageXml>(manifest2).expect_err("Should not have parsed!");
        }
    }

    #[test]
    fn parse_empty() {
        let manifest = r#"<?xml version="1.0"?>
            <?xml-model href="http://download.ros.org/schema/package_format3.xsd" schematypens="http://www.w3.org/2001/XMLSchema"?>
            <package format="3">
              <name>zzz_package</name>
              <version>1.0.0</version>
              <description>This is a cmake package</description>
              <maintainer email="foo@bar.com">Foo Bar</maintainer>
              <license>MIT</license>

              <buildtool_depend>ament_cmake</buildtool_depend>
              <export>
                <build_type>ament_cmake</build_type>
              </export>
            </package>
            "#;
        let parsed: PackageXml = from_str(manifest).unwrap();
        assert_eq!(parsed.name, "zzz_package");
        assert!(parsed.depend.is_empty());
        assert!(parsed.build_depend.is_empty());
        assert!(parsed.test_depend.is_empty());
        assert!(parsed.exec_depend.is_empty());
    }

    #[test]
    fn parses_package() {
        let manifest = r#"<?xml version="1.0"?>
            <?xml-model href="http://download.ros.org/schema/package_format3.xsd" schematypens="http://www.w3.org/2001/XMLSchema"?>
            <package format="3">
              <name>zzz_package</name>
              <version>1.0.0</version>
              <description>This is a cmake package</description>
              <maintainer email="foo@bar.com">Foo Bar</maintainer>
              <license>MIT</license>

              <buildtool_depend>ament_cmake</buildtool_depend>

              <depend>dep1</depend>
              <depend>dep2</depend>

              <build_depend>build_dep1</build_depend>
              <build_depend>build_dep2</build_depend>

              <test_depend>test_dep1</test_depend>
              <test_depend>test_dep2</test_depend>

              <exec_depend>exec_dep1</exec_depend>
              <exec_depend>exec_dep2</exec_depend>

              <export>
                <build_type>ament_cmake</build_type>
              </export>
            </package>
            "#;
        let parsed: PackageXml = from_str(manifest).unwrap();
        assert_eq!(parsed.name, "zzz_package");
        assert_eq!(parsed.depend, ["dep1", "dep2"]);
        assert_eq!(parsed.build_depend, ["build_dep1", "build_dep2"]);
        assert_eq!(parsed.test_depend, ["test_dep1", "test_dep2"]);
        assert_eq!(parsed.exec_depend, ["exec_dep1", "exec_dep2"]);
    }
}
