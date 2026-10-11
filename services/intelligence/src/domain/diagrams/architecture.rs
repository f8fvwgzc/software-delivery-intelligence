//! Architecture: the product's components (packages, module directories and
//! significant files), data stores and external systems with the calls
//! between them. Groups are gitdiagram-style roles (bootstrap, execution,
//! integrations, monitoring, state) when the code names enough of them,
//! else the top-level directories, plus "Data" and "External".

use std::collections::{BTreeMap, HashMap, HashSet};

use sdi_proto::knowledge::v1::NodeKind;

use super::{
    DiagramInput, add_graph_edges,
    catalog::{Catalog, humanize_dir, node_id},
    components::{Components, Role},
    evidence::{self, external_hosts},
    model::{
        DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec, MAX_EDGES, MAX_NODES, kind, style,
    },
};

pub const GROUP_DATA: &str = "data";
pub const GROUP_EXTERNAL: &str = "external";
pub const ACTOR_USER: &str = "actor_user";
pub const ACTOR_OPERATOR: &str = "actor_operator";
pub const ACTOR_CI: &str = "actor_ci";

/// Roles are used as groups when the components name at least this many.
const MIN_ROLES: usize = 3;

pub fn build(input: &DiagramInput<'_>) -> DiagramSpec {
    let catalog = Catalog::new(input);
    let mut components = Components::build(&catalog);
    let mut spec = DiagramSpec::new("LR");

    let stores: Vec<_> = catalog.of_kind(NodeKind::Datastore).collect();
    let externals: Vec<_> = catalog.of_kind(NodeKind::ExternalApi).collect();
    let hosts = external_hosts(&catalog);
    let actors = actor_count(&catalog, &components);
    let reserved = stores.len() + externals.len() + hosts.len() + actors;
    components.collapse_to(MAX_NODES.saturating_sub(reserved).max(4));

    // The binary the operator runs is bootstrap when nothing else names it.
    if let Some(principal) = principal_service(&components).map(|c| c.id.clone())
        && let Some(component) = components.items.iter_mut().find(|c| c.id == principal)
        && component.role == Role::Other
        && !serves_http(&catalog, &component.package)
    {
        component.role = Role::Bootstrap;
    }
    let role_mode = components.roles().len() >= MIN_ROLES;
    if role_mode {
        add_role_groups(&components, &mut spec);
    } else {
        add_dir_groups(&catalog, &components, &mut spec);
    }
    for component in &components.items {
        let group_id = if role_mode {
            component.role.id().to_owned()
        } else {
            dir_group_id(&catalog, &component.package)
        };
        spec.add_node(DiagramNode {
            id: component.id.clone(),
            label: component.label.clone(),
            kind: component.kind.to_owned(),
            group_id,
            path: component.path.clone(),
            description: component.description.clone(),
            caption: component.caption.clone(),
        });
    }
    add_datastores(&catalog, &mut spec, role_mode);
    add_externals(&catalog, &mut spec, role_mode);
    for host in &hosts {
        spec.add_group(external_group(role_mode));
        spec.add_node(DiagramNode {
            id: host.id.clone(),
            label: host.label.clone(),
            kind: kind::EXTERNAL.to_owned(),
            group_id: external_group(role_mode).id,
            path: String::new(),
            description: format!("External system reached at {}", host.host),
            caption: host.host.clone(),
        });
    }
    add_actors(&catalog, &components, &mut spec);

    // Import-graph edges carry the most specific verbs and cite the importer.
    for edge in &components.edges {
        spec.add_edge(DiagramEdge {
            id: String::new(),
            source: edge.source.clone(),
            target: edge.target.clone(),
            label: edge.label.clone(),
            style: edge.style.clone(),
            dashed: false,
            evidence_path: edge.evidence_path.clone(),
        });
    }
    // Sampled-file evidence (datastore drivers, sibling URLs) lands on the
    // component that owns the file.
    for found in evidence::detect(&catalog) {
        let mut edge = found.edge();
        if let Some(component) = components.component_of(&found.evidence_path) {
            edge.source = component;
        }
        spec.add_edge(edge);
    }
    for host in &hosts {
        for (file, _) in &host.callers {
            let source = components
                .component_of(file)
                .or_else(|| catalog.owner_of_path(file).map(|n| node_id(&n.key)));
            if let Some(source) = source {
                spec.add_edge(DiagramEdge {
                    id: String::new(),
                    source,
                    target: host.id.clone(),
                    label: "calls".to_owned(),
                    style: style::CALL.to_owned(),
                    dashed: false,
                    evidence_path: file.clone(),
                });
            }
        }
    }
    add_graph_edges(&catalog, &mut spec);
    add_inferred_ui_calls(&mut spec);
    drop_isolated_modules(&mut spec);
    let packages: HashMap<String, String> = components
        .items
        .iter()
        .map(|c| (c.id.clone(), c.package.clone()))
        .collect();
    trim_edges(&mut spec, &packages);
    spec.prune_groups();
    spec
}

/// A library or module component nothing calls and that calls nothing is
/// noise on a runtime map; services, UIs, stores and externals stay as facts.
fn drop_isolated_modules(spec: &mut DiagramSpec) {
    let connected: HashSet<String> = spec
        .edges
        .iter()
        .flat_map(|e| [e.source.clone(), e.target.clone()])
        .collect();
    spec.nodes.retain(|n| {
        connected.contains(&n.id)
            || !(n.kind == kind::MODULE || n.id.starts_with("dir_") || n.id.starts_with("file_"))
    });
}

/// The architecture map narrowed to what the prompt names: nodes whose
/// label, path or description mention a prompt word, plus their direct
/// neighbours. An empty or unmatched prompt yields the full map.
pub fn build_focused(input: &DiagramInput<'_>, prompt: &str) -> DiagramSpec {
    let mut spec = build(input);
    let words: Vec<String> = prompt
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| {
            !matches!(
                w.as_str(),
                "the" | "and" | "for" | "with" | "show" | "flow" | "diagram" | "map"
            )
        })
        .collect();
    if words.is_empty() {
        return spec;
    }
    let matched: HashSet<String> = spec
        .nodes
        .iter()
        .filter(|n| {
            let hay =
                format!("{} {} {} {}", n.label, n.path, n.description, n.caption).to_lowercase();
            words.iter().any(|w| hay.contains(w))
        })
        .map(|n| n.id.clone())
        .collect();
    if matched.is_empty() {
        return spec;
    }
    let keep: HashSet<String> = spec
        .edges
        .iter()
        .filter(|e| matched.contains(&e.source) || matched.contains(&e.target))
        .flat_map(|e| [e.source.clone(), e.target.clone()])
        .chain(matched.iter().cloned())
        .collect();
    spec.nodes.retain(|n| keep.contains(&n.id));
    spec.edges
        .retain(|e| keep.contains(&e.source) && keep.contains(&e.target));
    spec.prune_groups();
    spec
}

fn add_role_groups(components: &Components, spec: &mut DiagramSpec) {
    let used: HashSet<Role> = components.items.iter().map(|c| c.role).collect();
    for role in Role::ALL {
        if used.contains(&role) {
            spec.add_group(DiagramGroup {
                id: role.id().to_owned(),
                label: role.label().to_owned(),
                description: role.description().to_owned(),
                path: String::new(),
            });
        }
    }
}

fn dir_group_id(catalog: &Catalog<'_>, package_key: &str) -> String {
    let dir = catalog
        .node_by_key(package_key)
        .map(|n| catalog.top_dir(n))
        .unwrap_or_default();
    if dir.is_empty() {
        "root".to_owned()
    } else {
        format!("dir_{}", node_id(&dir))
    }
}

fn add_dir_groups(catalog: &Catalog<'_>, components: &Components, spec: &mut DiagramSpec) {
    let mut dirs: BTreeMap<String, usize> = BTreeMap::new();
    for component in &components.items {
        let dir = catalog
            .node_by_key(&component.package)
            .map(|n| catalog.top_dir(n))
            .unwrap_or_default();
        *dirs.entry(dir).or_default() += 1;
    }
    let mut ordered: Vec<(String, usize)> = dirs.into_iter().collect();
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (dir, _) in ordered {
        spec.add_group(DiagramGroup {
            id: if dir.is_empty() {
                "root".to_owned()
            } else {
                format!("dir_{}", node_id(&dir))
            },
            label: humanize_dir(&dir),
            description: if dir.is_empty() {
                "Packages at the repository root".to_owned()
            } else {
                format!("Packages under {dir}/")
            },
            path: dir.clone(),
        });
    }
}

fn data_group(role_mode: bool) -> DiagramGroup {
    if role_mode {
        DiagramGroup {
            id: Role::State.id().to_owned(),
            label: Role::State.label().to_owned(),
            description: Role::State.description().to_owned(),
            path: String::new(),
        }
    } else {
        DiagramGroup {
            id: GROUP_DATA.to_owned(),
            label: "Data".to_owned(),
            description: "Databases, caches and queues the services use".to_owned(),
            path: String::new(),
        }
    }
}

fn external_group(role_mode: bool) -> DiagramGroup {
    if role_mode {
        DiagramGroup {
            id: Role::Integrations.id().to_owned(),
            label: Role::Integrations.label().to_owned(),
            description: Role::Integrations.description().to_owned(),
            path: String::new(),
        }
    } else {
        DiagramGroup {
            id: GROUP_EXTERNAL.to_owned(),
            label: "External".to_owned(),
            description: "Third-party APIs the code talks to".to_owned(),
            path: String::new(),
        }
    }
}

pub fn add_datastores(catalog: &Catalog<'_>, spec: &mut DiagramSpec, role_mode: bool) {
    let stores: Vec<_> = catalog.of_kind(NodeKind::Datastore).collect();
    if stores.is_empty() {
        return;
    }
    let group = spec.add_group(data_group(role_mode));
    for node in stores {
        if let Some(diagram_node) = catalog.diagram_node(node, &group) {
            spec.add_node(diagram_node);
        }
    }
}

pub fn add_externals(catalog: &Catalog<'_>, spec: &mut DiagramSpec, role_mode: bool) {
    let externals: Vec<_> = catalog.of_kind(NodeKind::ExternalApi).collect();
    if externals.is_empty() {
        return;
    }
    let group = spec.add_group(external_group(role_mode));
    for node in externals {
        if let Some(diagram_node) = catalog.diagram_node(node, &group) {
            spec.add_node(diagram_node);
        }
    }
}

/// How many actor nodes `add_actors` will add.
fn actor_count(catalog: &Catalog<'_>, components: &Components) -> usize {
    let mut n = 0;
    if entry_targets(catalog, components).is_some() {
        n += 1;
    }
    if ci_workflow(catalog).is_some() {
        n += 1;
    }
    n
}

/// Whether a package serves HTTP: routes in its sampled files or a backend
/// web framework on its graph node.
pub(crate) fn serves_http(catalog: &Catalog<'_>, package_key: &str) -> bool {
    let Some(node) = catalog.node_by_key(package_key) else {
        return false;
    };
    let package_dir = catalog.dir_of(node);
    catalog.input.files.iter().any(|(p, text)| {
        super::catalog::within(p, &package_dir) && !evidence::routes_in(text, 1).is_empty()
    }) || catalog.frameworks_of(node).iter().any(|f| {
        f.contains("axum")
            || f.contains("express")
            || f.contains("fastapi")
            || f.contains("django")
            || f.contains("flask")
            || f.contains("spring")
    })
}

/// The backend service with an entry point that the operator starts or
/// calls: the best connected protected service root.
pub(crate) fn principal_service(components: &Components) -> Option<&super::components::Component> {
    let mut services: Vec<&super::components::Component> = components
        .items
        .iter()
        .filter(|c| c.is_root && c.protected && c.kind == kind::SERVICE)
        .collect();
    services.sort_by_key(|c| (std::cmp::Reverse(components.degree(&c.id)), c.id.clone()));
    services.first().copied()
}

/// The entry edges of the human actor: every UI is opened; the principal
/// service is run (a CLI) or called (an HTTP server). The actor is "User"
/// when only interfaces exist, "Operator" when a binary is started.
/// `(actor label, caption, [(target id, verb)])`.
type EntryTargets = (&'static str, &'static str, Vec<(String, &'static str)>);

fn entry_targets(catalog: &Catalog<'_>, components: &Components) -> Option<EntryTargets> {
    let mut targets: Vec<(String, &'static str)> = components
        .items
        .iter()
        .filter(|c| c.is_root && c.kind == kind::UI)
        .map(|c| (c.id.clone(), "opens"))
        .collect();
    let mut operator = false;
    if let Some(service) = principal_service(components) {
        if serves_http(catalog, &service.package) {
            // A UI already fronts an HTTP service; the operator calls it only when there is none.
            if targets.is_empty() {
                targets.push((service.id.clone(), "calls"));
            }
        } else {
            targets.push((service.id.clone(), "runs"));
            operator = true;
        }
    }
    if targets.is_empty() {
        return None;
    }
    Some(if operator {
        ("Operator", "CLI")
    } else if targets.iter().all(|(_, v)| *v == "opens") {
        ("User", "browser")
    } else {
        ("Operator", "HTTP")
    })
    .map(|(label, caption)| (label, caption, targets))
}

/// The workflow file of a pipeline that deploys or releases (or, failing
/// that, builds an image), if any.
fn ci_workflow(catalog: &Catalog<'_>) -> Option<String> {
    let mut candidates: Vec<(u8, String)> = catalog
        .edges()
        .filter_map(|e| {
            let source = catalog.node_by_id(&e.source_id)?;
            if source.kind() != NodeKind::Pipeline {
                return None;
            }
            let rank = match e.relation.as_str() {
                "DEPLOYS" | "RELEASES" => 0,
                "BUILDS" => 1,
                _ => return None,
            };
            let path = catalog.path_of(source);
            (!path.is_empty()).then_some((rank, path))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next().map(|(_, p)| p)
}

fn add_actors(catalog: &Catalog<'_>, components: &Components, spec: &mut DiagramSpec) {
    if let Some((label, caption, targets)) = entry_targets(catalog, components) {
        let id = if label == "User" {
            ACTOR_USER
        } else {
            ACTOR_OPERATOR
        };
        spec.add_node(DiagramNode {
            id: id.to_owned(),
            label: label.to_owned(),
            kind: kind::ACTOR.to_owned(),
            group_id: String::new(),
            path: String::new(),
            description: if label == "User" {
                "A person using the product in a browser".to_owned()
            } else {
                "Whoever starts and drives the product".to_owned()
            },
            caption: caption.to_owned(),
        });
        for (target, verb) in targets {
            spec.add_edge(DiagramEdge {
                id: String::new(),
                source: id.to_owned(),
                target,
                label: verb.to_owned(),
                style: style::CALL.to_owned(),
                dashed: true,
                evidence_path: String::new(),
            });
        }
    }
    if let Some(workflow) = ci_workflow(catalog) {
        spec.add_node(DiagramNode {
            id: ACTOR_CI.to_owned(),
            label: "CI".to_owned(),
            kind: kind::ACTOR.to_owned(),
            group_id: String::new(),
            path: workflow.clone(),
            description: "The pipeline that builds and releases the product".to_owned(),
            caption: "pipeline".to_owned(),
        });
        // CI builds the product's protected entry packages.
        let targets: Vec<String> = components
            .items
            .iter()
            .filter(|c| c.is_root && c.protected && c.kind != kind::MODULE)
            .map(|c| c.id.clone())
            .collect();
        for target in targets {
            spec.add_edge(DiagramEdge {
                id: String::new(),
                source: ACTOR_CI.to_owned(),
                target,
                label: "builds & releases".to_owned(),
                style: style::CALL.to_owned(),
                dashed: false,
                evidence_path: workflow.clone(),
            });
        }
    }
}

/// A UI with no observed call to a backend almost certainly talks to the
/// only one there is; drawn dashed because nothing in the sample shows it.
pub fn add_inferred_ui_calls(spec: &mut DiagramSpec) {
    let backends: Vec<String> = spec
        .nodes
        .iter()
        .filter(|n| {
            n.kind == kind::SERVICE && !n.id.starts_with("dir_") && !n.id.starts_with("file_")
        })
        .map(|n| n.id.clone())
        .collect();
    let [only] = backends.as_slice() else {
        return;
    };
    let uis: Vec<String> = spec
        .nodes
        .iter()
        .filter(|n| n.kind == kind::UI && !n.id.starts_with("dir_") && !n.id.starts_with("file_"))
        .map(|n| n.id.clone())
        .collect();
    for ui in uis {
        let has_call = spec.edges.iter().any(|e| {
            e.source == ui
                && e.style == style::CALL
                && !e.target.starts_with("dir_")
                && !e.target.starts_with("file_")
        });
        if !has_call {
            spec.add_edge(DiagramEdge {
                id: String::new(),
                source: ui,
                target: only.clone(),
                label: "calls".to_owned(),
                style: style::CALL.to_owned(),
                dashed: true,
                evidence_path: String::new(),
            });
        }
    }
}

/// Keeps the 48 most telling edges: actor edges, then edges to stores and
/// external systems, then cross-package calls with a specific verb, then
/// the rest, generic "uses" and inferred ones last.
fn trim_edges(spec: &mut DiagramSpec, packages: &HashMap<String, String>) {
    if spec.edges.len() <= MAX_EDGES {
        return;
    }
    let kinds: HashMap<&str, &str> = spec
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.kind.as_str()))
        .collect();
    let rank = |e: &DiagramEdge| -> (u8, bool) {
        let actor = e.source.starts_with("actor_");
        let boundary = matches!(
            kinds.get(e.target.as_str()).copied(),
            Some(kind::DATASTORE | kind::QUEUE | kind::EXTERNAL)
        );
        let cross = packages.get(&e.source) != packages.get(&e.target);
        let generic = matches!(e.label.as_str(), "uses" | "imports" | "configures");
        let class = if actor {
            0
        } else if e.dashed {
            6
        } else if boundary {
            1
        } else if cross && !generic {
            2
        } else if cross {
            3
        } else if !generic {
            4
        } else {
            5
        };
        (class, e.evidence_path.is_empty())
    };
    let mut ranked: Vec<(usize, (u8, bool))> = spec
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| (i, rank(e)))
        .collect();
    ranked.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let keep: HashSet<usize> = ranked.iter().take(MAX_EDGES).map(|(i, _)| *i).collect();
    let mut index = 0;
    spec.edges.retain(|_| {
        let kept = keep.contains(&index);
        index += 1;
        kept
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::diagrams::{
        fixtures::{self, input, tree},
        model::MAX_GROUPS,
    };

    #[test]
    fn groups_come_from_top_level_dirs_plus_data_and_external() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let labels: Vec<&str> = spec.groups.iter().map(|g| g.label.as_str()).collect();
        assert!(labels.contains(&"Services"), "{labels:?}");
        assert!(labels.contains(&"Web"), "{labels:?}");
        assert!(labels.contains(&"Crates"), "{labels:?}");
        assert!(labels.contains(&"Data"), "{labels:?}");
        assert!(labels.contains(&"External"), "{labels:?}");
        assert!((3..=MAX_GROUPS).contains(&spec.groups.len()), "{labels:?}");
        // Groups carry the directory they stand for.
        assert_eq!(spec.group("dir_services").unwrap().path, "services");

        // The e2e package under tests/ is machinery, not product.
        assert!(!spec.has_node("library_e2e"));
        assert!(!spec.nodes.iter().any(|n| n.kind == kind::JOB));

        // Kinds and captions from facts.
        let web = spec.node("service_web").unwrap();
        assert_eq!(web.kind, kind::UI);
        assert_eq!(web.caption, "React");
        assert_eq!(web.path, "web/src/main.tsx");
        let api = spec.node("service_api").unwrap();
        assert_eq!(api.kind, kind::SERVICE);
        assert_eq!(api.caption, "axum");
        assert_eq!(api.path, "services/api/src/main.rs");
        let quant = spec.node("service_quant").unwrap();
        assert_eq!(quant.caption, "FastAPI");
        assert_eq!(quant.path, "services/quant/app/main.py");
        let pg = spec.node("datastore_postgres").unwrap();
        assert_eq!(pg.kind, kind::DATASTORE);
        assert_eq!(
            pg.caption, "postgres:16",
            "the image, since the label is the engine"
        );
        assert_eq!(spec.node("datastore_redis").unwrap().caption, "");
        assert_eq!(spec.node("datastore_kafka").unwrap().kind, kind::QUEUE);
        assert_eq!(
            spec.node("external_api_stripe").unwrap().kind,
            kind::EXTERNAL
        );
        assert_eq!(spec.node("library_shared").unwrap().kind, kind::MODULE);
        assert_eq!(
            spec.node("library_shared").unwrap().path,
            "crates/shared/src/lib.rs"
        );

        // Module-level components from the import graph.
        assert!(
            spec.nodes.len() >= 10,
            "{:?}",
            spec.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
        assert!(
            spec.edges.len() >= 10,
            "{:?}",
            spec.edges
                .iter()
                .map(|e| (&e.source, &e.target, &e.label))
                .collect::<Vec<_>>()
        );
        let routes = spec
            .node("dir_services_api_src_routes")
            .expect("routes dir component");
        assert_eq!(routes.label, "HTTP server");
        assert_eq!(routes.group_id, "dir_services");
        let db = spec
            .node("dir_services_api_src_db")
            .expect("db dir component");
        assert_eq!(db.label, "Api store");
        let client = spec
            .node("file_web_src_lib_api_ts")
            .expect("api.ts has fan-in 3");
        assert_eq!(client.label, "API client");
        assert_eq!(client.caption, "api.ts");
        assert!(spec.has_node("dir_web_src_features_views"));
        // The user reaches the UI.
        let user = spec.edges.iter().find(|e| e.source == ACTOR_USER).unwrap();
        assert_eq!(user.target, "service_web");
        assert!(user.dashed);
        // CI builds and releases, cited by the workflow.
        let ci = spec.edges.iter().find(|e| e.source == ACTOR_CI).unwrap();
        assert_eq!(ci.evidence_path, ".github/workflows/deploy.yml");
    }

    #[test]
    fn edges_cite_the_callers_file_and_inferred_ones_are_dashed() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        let find = |s: &str, t: &str| spec.edges.iter().find(|e| e.source == s && e.target == t);

        // Graph edge api -> postgres upgraded by the sqlx evidence in main.rs.
        let api_pg = find("service_api", "datastore_postgres").expect("api -> postgres");
        assert!(!api_pg.dashed);
        assert_eq!(api_pg.style, style::DATA);
        assert!(
            api_pg.evidence_path.starts_with("services/api/"),
            "{}",
            api_pg.evidence_path
        );

        // Regex evidence: the API calls quant by its base URL; the import
        // graph shows it uses the shared crate.
        let api_quant = find("service_api", "service_quant").expect("api -> quant");
        assert_eq!(api_quant.evidence_path, "services/api/src/main.rs");
        assert_eq!(api_quant.label, "calls");
        let api_shared = find("service_api", "library_shared").expect("api -> shared");
        assert_eq!(api_shared.label, "uses");
        assert_eq!(api_shared.evidence_path, "services/api/src/main.rs");
        // Import-graph verbs: main.rs mounts the routes, routes read the store.
        let api_routes = find("service_api", "dir_services_api_src_routes").expect("api -> routes");
        assert_eq!(api_routes.label, "serves");
        let routes_db =
            find("dir_services_api_src_routes", "dir_services_api_src_db").expect("routes -> db");
        assert_eq!(routes_db.style, style::DATA);
        assert_eq!(routes_db.evidence_path, "services/api/src/routes/views.rs");
        let db_pg = find("dir_services_api_src_db", "datastore_postgres").expect("db -> postgres");
        assert_eq!(db_pg.evidence_path, "services/api/src/db/mod.rs");
        // Redis via the cache module, which folds into the api root.
        let api_redis = find("service_api", "datastore_redis").expect("api -> redis");
        assert!(!api_redis.dashed);
        // Kafka: graph edge, manifest as evidence.
        let api_kafka = find("service_api", "datastore_kafka").expect("api -> kafka");
        assert_eq!(api_kafka.label, "publishes");
        assert_eq!(api_kafka.style, style::ASYNC);
        // quant -> postgres from asyncpg in main.py.
        let quant_pg = find("service_quant", "datastore_postgres").expect("quant -> postgres");
        assert_eq!(quant_pg.evidence_path, "services/quant/app/main.py");
        // Stripe from the graph: manifest cited.
        let stripe = find("service_api", "external_api_stripe").expect("api -> stripe");
        assert_eq!(stripe.evidence_path, "services/api/Cargo.toml");
        // The expired web -> kafka edge never appears.
        assert!(find("service_web", "datastore_kafka").is_none());
        // Structural CONTAINS edges never appear.
        assert!(spec.edges.iter().all(|e| e.label != "contains"));
        // Every edge has evidence or is dashed; verbs are short.
        for edge in &spec.edges {
            assert_eq!(edge.dashed, edge.evidence_path.is_empty(), "{edge:?}");
            assert!(edge.label.split_whitespace().count() <= 3, "{edge:?}");
        }
    }

    #[test]
    fn without_files_graph_edges_cite_manifests_and_a_lone_ui_call_is_dashed() {
        let files = BTreeMap::new();
        let tree = tree();
        let mut ctx = fixtures::context();
        // Drop quant so the web app has exactly one backend to talk to.
        if let Some(graph) = ctx.graph.as_mut() {
            graph.nodes.retain(|n| n.id != "n-quant");
            graph.edges.retain(|e| e.source_id != "n-quant");
        }
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let web_api = spec
            .edges
            .iter()
            .find(|e| e.source == "service_web" && e.target == "service_api")
            .expect("web -> api inferred");
        assert!(web_api.dashed);
        assert!(web_api.evidence_path.is_empty());
        let api_pg = spec
            .edges
            .iter()
            .find(|e| e.source == "service_api" && e.target == "datastore_postgres")
            .unwrap();
        assert_eq!(api_pg.evidence_path, "services/api/Cargo.toml");
        assert!(!api_pg.dashed);
    }

    #[test]
    fn focused_build_keeps_the_named_nodes_and_their_neighbours() {
        let files = fixtures::files();
        let tree = tree();
        let ctx = fixtures::context();
        let spec = build_focused(
            &input(&ctx, &tree, &files, "the quant pricing"),
            "the quant pricing",
        );
        spec.validate().unwrap();
        assert!(spec.has_node("service_quant"));
        assert!(spec.has_node("datastore_postgres"));
        assert!(spec.has_node("service_api"));
        assert!(!spec.has_node("datastore_redis"));
        let full = build_focused(&input(&ctx, &tree, &files, ""), "");
        assert!(full.nodes.len() > spec.nodes.len());
    }

    #[test]
    fn symphony_like_workspace_reaches_gitdiagram_richness() {
        let files = fixtures::symphony_files();
        let tree = fixtures::symphony_tree();
        let ctx = fixtures::symphony_context();
        let spec = build(&input(&ctx, &tree, &files, ""));
        spec.validate().unwrap();
        let ids: Vec<&str> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
        let labels: Vec<(&str, &str, &str)> = spec
            .edges
            .iter()
            .map(|e| (e.source.as_str(), e.label.as_str(), e.target.as_str()))
            .collect();
        assert!(spec.nodes.len() >= 14, "{ids:?}");
        assert!(spec.edges.len() >= 18, "{labels:?}");
        assert!(spec.groups.len() >= 4, "{:?}", spec.groups);
        assert!(spec.nodes.len() <= MAX_NODES && spec.edges.len() <= MAX_EDGES);
        // Role groups, since the code names bootstrap, execution, integrations, monitoring and state.
        let group_ids: Vec<&str> = spec.groups.iter().map(|g| g.id.as_str()).collect();
        assert!(group_ids.contains(&Role::Execution.id()), "{group_ids:?}");
        assert!(group_ids.contains(&Role::State.id()), "{group_ids:?}");
        assert!(
            group_ids.contains(&Role::Integrations.id()),
            "{group_ids:?}"
        );
        for node in &spec.nodes {
            if !node.path.is_empty() {
                assert!(
                    tree.iter().any(|e| e.path == node.path),
                    "{} -> {}",
                    node.id,
                    node.path
                );
            }
            let words = node.label.split_whitespace().count();
            assert!((1..=4).contains(&words), "{:?}", node.label);
        }
        for edge in &spec.edges {
            assert_eq!(edge.dashed, edge.evidence_path.is_empty(), "{edge:?}");
            let words = edge.label.split_whitespace().count();
            assert!((1..=3).contains(&words), "{:?}", edge.label);
        }
        // The operator runs the binary; the binary starts the runtime, the
        // orchestrator records runs, the trackers call Linear, the store
        // reads/writes SQLite.
        assert!(
            labels.contains(&(ACTOR_OPERATOR, "runs", "service_symphony")),
            "{labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(s, l, t)| *s == "file_crates_symphony_src_app_rs"
                    && *l == "starts"
                    && t.contains("runtime")),
            "{labels:?}"
        );
        assert!(
            labels.contains(&(ACTOR_OPERATOR, "opens", "service_symphony_web")),
            "{labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(_, l, t)| *l == "records" && t.contains("store")),
            "{labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(_, l, t)| *l == "calls" && *t == "external_linear"),
            "{labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(s, _, t)| s.contains("store") && *t == "datastore_sqlite"),
            "{labels:?}"
        );
        assert!(
            labels
                .iter()
                .any(|(s, l, _)| *s == ACTOR_CI && *l == "builds & releases"),
            "{labels:?}"
        );
    }
}
