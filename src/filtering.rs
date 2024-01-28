use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    path::PathBuf,
};

use clap::ValueEnum;

use crate::parsing::Entry;

#[derive(ValueEnum, PartialOrd, PartialEq, Eq, Ord, Clone, Default, Debug)]
pub enum DepType {
    #[default]
    All,
    Build,
    Exec,
    Test,
}

impl DepType {
    pub fn matches(&self, b: &DepType) -> bool {
        *self == DepType::All || *b == DepType::All || *self == *b
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Dependency {
    name: String,
    dep_type: DepType,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Package {
    pub name: String,
    pub path: PathBuf,
    deps: Vec<Dependency>,
}

impl Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.path.display())
    }
}

impl From<Entry> for Package {
    fn from(entry: Entry) -> Self {
        let mut deps: Vec<_> = Vec::with_capacity(
            entry.pkg.depend.len()
                + entry.pkg.build_depend.len()
                + entry.pkg.exec_depend.len()
                + entry.pkg.test_depend.len(),
        );
        for x in entry.pkg.depend.iter() {
            deps.push(Dependency {
                name: x.clone(),
                dep_type: DepType::All,
            });
        }
        for x in entry.pkg.build_depend.iter() {
            deps.push(Dependency {
                name: x.clone(),
                dep_type: DepType::Build,
            });
        }
        for x in entry.pkg.exec_depend.iter() {
            deps.push(Dependency {
                name: x.clone(),
                dep_type: DepType::Exec,
            });
        }
        for x in entry.pkg.test_depend.iter() {
            deps.push(Dependency {
                name: x.clone(),
                dep_type: DepType::Test,
            });
        }

        Self {
            name: entry.pkg.name,
            path: entry.path,
            deps,
        }
    }
}

pub type DepFilter = dyn Fn(&Dependency) -> bool;

impl Dependency {
    pub fn all(_candidate: &Dependency) -> bool {
        true
    }

    pub fn build(candidate: &Dependency) -> bool {
        candidate.dep_type.matches(&DepType::Build)
    }

    pub fn matcher(mut types: Vec<DepType>) -> impl Fn(&Dependency) -> bool {
        types.sort();
        types.dedup();
        move |candidate: &Dependency| types.iter().any(|t| t.matches(&candidate.dep_type))
    }
}

fn remove_recursively(unused: &mut HashMap<&str, &Package>, pkg: &str, filter: &DepFilter) {
    if let Some(v) = unused.remove(pkg) {
        for p in v.deps.iter().filter(|x| filter(x)) {
            remove_recursively(unused, &p.name, filter);
        }
    }
}

pub fn find_unused_pkgs(
    build_space: &[Package],
    upstream: &[Package],
    filter: &DepFilter,
) -> Vec<Package> {
    let mut used = HashSet::<&str>::new();
    for p in build_space {
        for dep in p.deps.iter().filter(|x| filter(x)) {
            used.insert(&dep.name);
        }
    }

    let mut unused = HashMap::<&str, &Package>::new();

    for p in upstream {
        // necessary in case the workspaces overlap.
        if !build_space.contains(p) {
            unused.insert(&p.name, p);
        }
    }

    for &p in used.iter() {
        remove_recursively(&mut unused, p, filter);
    }

    unused
        .values()
        .map(|&x| x.clone())
        .collect::<Vec<Package>>()
}

#[cfg(test)]
mod tests {
    use crate::filtering::*;

    fn test_package(name: &str, deps: &[&str]) -> Package {
        Package {
            name: name.to_string(),
            path: "name".into(),
            deps: deps
                .iter()
                .map(|n| Dependency {
                    name: n.to_string(),
                    dep_type: DepType::All,
                })
                .collect(),
        }
    }

    #[test]
    fn empty_needs_nothing() {
        let res = find_unused_pkgs(&[], &[], &Dependency::all);
        assert_eq!(res, [])
    }

    #[test]
    fn normal_dependencies() {
        let ws = vec![test_package("test", &["a"])];
        let a = test_package("a", &[]);
        let b = test_package("b", &[]);
        let upstream = vec![a, b.clone()];
        let res = find_unused_pkgs(&ws, &upstream, &Dependency::all);
        assert_eq!(res, [b]);
    }

    #[test]
    fn transitive() {
        let ws = vec![Package {
            name: "test".to_string(),
            path: ".".into(),
            deps: vec![
                Dependency {
                    name: "a".into(),
                    dep_type: DepType::Build,
                },
                Dependency {
                    name: "other".into(),
                    dep_type: DepType::All,
                },
            ],
        }];
        let a = Package {
            name: "a".to_string(),
            path: ".".into(),
            deps: vec![Dependency {
                name: "b".into(),
                dep_type: DepType::Build,
            }],
        };
        let b = Package {
            name: "b".to_string(),
            path: ".".into(),
            deps: vec![],
        };
        let res = find_unused_pkgs(&ws, &[a, b], &Dependency::all);
        assert_eq!(res, []);
    }

    #[test]
    fn filters() {
        let ws = vec![Package {
            name: "test".to_string(),
            path: ".".into(),
            deps: vec![Dependency {
                name: "a".into(),
                dep_type: DepType::Exec,
            }],
        }];
        let a = Package {
            name: "a".to_string(),
            path: ".".into(),
            deps: vec![Dependency {
                name: "b".into(),
                dep_type: DepType::Build,
            }],
        };
        let res = find_unused_pkgs(&ws, &[a.clone()], &Dependency::build);
        assert_eq!(res, [a]);
    }

    #[test]
    fn overlaps() {
        let a = test_package("a", &[]);
        let b = test_package("b", &[]);
        let c = test_package("c", &[]);
        let d = test_package("d", &[]);
        let ws = vec![a, b, c, d];
        let res = find_unused_pkgs(&ws, &ws, &Dependency::all);
        assert!(res.is_empty());
    }
}
