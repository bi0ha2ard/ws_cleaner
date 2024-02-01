use anyhow::{anyhow, Context, Result};
use std::{
    convert::identity,
    fs::{self, File},
    io::prelude::*,
    io::BufReader,
    path::{Path, PathBuf},
};
use xml::reader::{EventReader, XmlEvent};

use crate::filtering::{DepType, Dependency, Package};

enum SearchOutcome {
    Found(Package),
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
        return parse_package(&pkg_xml).map(Found);
    }
    Ok(Recurse {})
}

fn parse_contents(path: &Path, reader: impl Read) -> Result<Package> {
    let parser = EventReader::new(reader);

    let mut depth = 0;

    #[derive(PartialEq)]
    enum Pending {
        Name,
        Depend,
        BuildDepend,
        TestDepend,
        ExecDepend,
        Other,
    }

    fn tag_from_name(name: &str) -> Pending {
        match name {
            "name" => Pending::Name,
            "depend" => Pending::Depend,
            "build_depend" => Pending::BuildDepend,
            "test_depend" => Pending::TestDepend,
            "exec_depend" => Pending::ExecDepend,
            _ => Pending::Other,
        }
    }

    let mut pending = Pending::Other;

    let mut maybe_name = None;
    let mut deps = Vec::new();

    for e in parser {
        match e {
            Ok(XmlEvent::StartElement { name, .. }) => {
                if depth == 0 && name.local_name != "package" {
                    return Err(anyhow!("Expected 'package' as root element!"));
                }
                if depth == 1 {
                    pending = tag_from_name(name.local_name.as_str());
                }
                if depth > 1 && pending != Pending::Other {
                    return Err(anyhow!("Expected tag '{name}' at depth 1!"));
                }
                depth += 1;
            }
            Ok(XmlEvent::Characters(data)) => match pending {
                Pending::Name => {
                    maybe_name = Some(data);
                }
                Pending::Depend => {
                    deps.push(Dependency {
                        name: data,
                        dep_type: DepType::All,
                    });
                }
                Pending::BuildDepend => {
                    deps.push(Dependency {
                        name: data,
                        dep_type: DepType::Build,
                    });
                }
                Pending::TestDepend => {
                    deps.push(Dependency {
                        name: data,
                        dep_type: DepType::Test,
                    });
                }
                Pending::ExecDepend => {
                    deps.push(Dependency {
                        name: data,
                        dep_type: DepType::Exec,
                    });
                }
                Pending::Other => { /* ignored */ }
            },
            Ok(XmlEvent::EndElement { name }) => {
                if tag_from_name(name.local_name.as_str()) != pending {
                    // All the tags we care about are depth 1
                    return Err(anyhow!("Closing tag doesn't match opening tag!"));
                }
                depth -= 1;
            }
            Err(e) => {
                return Err(e.into());
            }
            // There's more: https://docs.rs/xml-rs/latest/xml/reader/enum.XmlEvent.html
            _ => {}
        }
    }

    let name = maybe_name.context("Field 'name' missing from package.xml")?;
    Ok(Package {
        name,
        path: path.to_path_buf(),
        deps,
    })
}

fn parse_package(xml_file: &PathBuf) -> Result<Package> {
    let context = || format!("While trying to parse '{}'", xml_file.display());
    let f = File::open(xml_file).with_context(context)?;
    // Prevent huge XML files blowing us up
    let reader = BufReader::new(f.take(1024 * 1024));

    parse_contents(xml_file, reader)
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
                results.push(entry);
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
        res.push(entry);
    }
    find_packages(dir, &mut res, true)?;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::parse_contents;
    use crate::filtering::{Dependency, Package};

    fn from_str(data: &str) -> anyhow::Result<Package> {
        parse_contents(&PathBuf::from("."), data.as_bytes())
    }

    fn dep(dep: &str) -> Dependency {
        Dependency {
            name: dep.to_string(),
            dep_type: crate::filtering::DepType::All,
        }
    }
    fn bdep(dep: &str) -> Dependency {
        Dependency {
            name: dep.to_string(),
            dep_type: crate::filtering::DepType::Build,
        }
    }
    fn tdep(dep: &str) -> Dependency {
        Dependency {
            name: dep.to_string(),
            dep_type: crate::filtering::DepType::Test,
        }
    }
    fn edep(dep: &str) -> Dependency {
        Dependency {
            name: dep.to_string(),
            dep_type: crate::filtering::DepType::Exec,
        }
    }

    #[test]
    fn fails_on_broken() {
        {
            let manifest1 = r#"<?xml version="1.0"?>
            <?xml-model href="http://download.ros.org/schema/package_format3.xsd" schematypens="http://www.w3.org/2001/XMLSchema"?>
            farts
            </package>
            "#;
            from_str(manifest1).expect_err("Should not have parsed!");
        }
        {
            let manifest2 = r#"nothing"#;
            from_str(manifest2).expect_err("Should not have parsed!");
        }
        {
            let manifest3 = r#"<package><name>foo</package>"#;
            from_str(manifest3).expect_err("Should not have parsed!");
        }
        {
            let manifest3 = r#"<package><foo><name>foo</name></foo></package>"#;
            from_str(manifest3).expect_err("Should not have parsed!");
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
        let parsed: Package = from_str(manifest).unwrap();
        assert_eq!(parsed.name, "zzz_package");
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
        let parsed: Package = from_str(manifest).unwrap();
        assert_eq!(parsed.name, "zzz_package");
        assert_eq!(
            parsed.deps,
            vec![
                dep("dep1"),
                dep("dep2"),
                bdep("build_dep1"),
                bdep("build_dep2"),
                tdep("test_dep1"),
                tdep("test_dep2"),
                edep("exec_dep1"),
                edep("exec_dep2")
            ]
        );
    }

    #[test]
    fn processes_unordered() {
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
              <build_depend>build_dep1</build_depend>
              <test_depend>test_dep1</test_depend>
              <exec_depend>exec_dep1</exec_depend>

              <depend>dep2</depend>
              <build_depend>build_dep2</build_depend>
              <test_depend>test_dep2</test_depend>
              <exec_depend>exec_dep2</exec_depend>

              <export>
                <build_type>ament_cmake</build_type>
              </export>
            </package>
            "#;
        let parsed: Package = from_str(manifest).unwrap();
        assert_eq!(
            parsed.deps,
            vec![
                dep("dep1"),
                bdep("build_dep1"),
                tdep("test_dep1"),
                edep("exec_dep1"),
                dep("dep2"),
                bdep("build_dep2"),
                tdep("test_dep2"),
                edep("exec_dep2")
            ]
        );
    }
}
