use clap::Parser;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Parser)]
#[command(name = "nnt")]
#[command(about = "Nym Network Topology - (c) 2026 Ch1ffr3punk", long_about = None)]
struct Cli {
    /// SOCKS5 proxy (e.g. 127.0.0.1:1080)
    #[arg(short, long)]
    proxy: Option<String>,

    /// Mixnodes: Node-ID, Country, IP, Hostname, Family
    #[arg(short = 'm', long, conflicts_with_all = &["gateways", "network_requesters"])]
    mixnodes: bool,

    /// Gateways: Node-ID, Country, IP, Hostname, Roles, Family
    #[arg(short = 'g', long, conflicts_with_all = &["mixnodes", "network_requesters"])]
    gateways: bool,

    /// Network Requesters: Node-ID, Country, IP, Family, Nym Address
    #[arg(short = 'n', long, conflicts_with_all = &["mixnodes", "gateways"])]
    network_requesters: bool,
}

const BASE_URL: &str = "https://validator.nymtech.net";
const VERIFIED_ENDPOINTS: &[&str] = &[
    "/api/v1/circulating-supply",
    "/api/v1/epoch/current",
    "/api/v1/nym-nodes/rewarded-set",
    "/api/v1/nym-nodes/described",
    "/api/v1/network/chain-status",
    "/api/v1/ecash/aggregated-coin-indices-signatures",
    "/api/v1/unstable/nym-nodes/skimmed/entry-gateways/all",
    "/api/v1/unstable/nym-nodes/skimmed/mixnodes/active",
    "/api/v1/status",
    "/api/v1/mixnodes",
    "/api/v1/gateways",
];

#[derive(Deserialize, Clone)]
struct NymNodesDescribed {
    data: Vec<Node>,
}

#[derive(Deserialize, Clone)]
struct Node {
    node_id: u32,
    description: NodeDescription,
}

#[derive(Deserialize, Clone)]
struct NodeDescription {
    auxiliary_details: AuxiliaryDetails,
    host_information: HostInfo,
    declared_role: DeclaredRole,
    #[serde(default)]
    network_requester: Option<NetworkRequester>,
    #[serde(default)]
    ip_packet_router: Option<IpPacketRouter>,
}

#[derive(Deserialize, Clone)]
struct AuxiliaryDetails {
    #[serde(default)]
    location: Option<String>,
}

#[derive(Deserialize, Clone)]
struct HostInfo {
    #[serde(default)]
    ip_address: Vec<String>,
    #[serde(default)]
    hostname: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
struct DeclaredRole {
    #[serde(default)]
    entry: bool,
    #[serde(default)]
    exit_ipr: bool,
    #[serde(default)]
    exit_nr: bool,
    #[serde(default)]
    mixnode: bool,
}

#[derive(Deserialize, Clone)]
struct NetworkRequester {
    address: String,
}

#[derive(Deserialize, Clone)]
struct IpPacketRouter {
    address: String,
}

// --- Node Families ---

#[derive(Deserialize, Clone)]
struct FamiliesResponse {
    data: Vec<NodeFamily>,
}

#[derive(Deserialize, Clone)]
struct NodeFamily {
    // id: u32,
    name: String,
    members: Vec<FamilyMember>,
}

#[derive(Deserialize, Clone)]
struct FamilyMember {
    node_id: u32,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut client_builder = reqwest::Client::builder();

    if let Some(proxy_input) = &cli.proxy {
        let proxy_url = if proxy_input.contains("://") {
            proxy_input.clone()
        } else {
            format!("socks5h://{}", proxy_input)
        };
        eprintln!("Using proxy: {}", proxy_url);
        client_builder = client_builder.proxy(reqwest::Proxy::all(&proxy_url)?);
    }

    let client = client_builder.build()?;

    if cli.mixnodes {
        let (nodes, family_map) = tokio::try_join!(
            fetch_nodes(&client),
            fetch_family_map(&client)
        )?;
        print_mixnodes(&nodes.data, &family_map);
        return Ok(());
    }

    if cli.gateways {
        let (nodes, family_map) = tokio::try_join!(
            fetch_nodes(&client),
            fetch_family_map(&client)
        )?;
        print_gateways(&nodes.data, &family_map);
        return Ok(());
    }

    if cli.network_requesters {
        let (nodes, family_map) = tokio::try_join!(
            fetch_nodes(&client),
            fetch_family_map(&client)
        )?;
        print_network_requesters(&nodes.data, &family_map);
        return Ok(());
    }

    // Default: alle Endpoints als JSON ausgeben
    eprintln!("Fetching all endpoints...");

    let mut collected_data = serde_json::Map::new();
    let total = VERIFIED_ENDPOINTS.len();

    for (i, endpoint) in VERIFIED_ENDPOINTS.iter().enumerate() {
        let url = format!("{}{}", BASE_URL, endpoint);
        eprintln!("[{}/{}] Fetching: {}", i + 1, total, url);

        match query_endpoint(&client, &url).await {
            Ok(data) => {
                let key = endpoint.trim_start_matches("/api/v1/").replace("/", "_");
                collected_data.insert(key, data);
                eprintln!("  OK");
            }
            Err(e) => {
                eprintln!("  ERROR: {}", e);
                let key = endpoint.trim_start_matches("/api/v1/").replace("/", "_");
                collected_data.insert(key, Value::Null);
            }
        }
    }

    let combined = Value::Object(collected_data);
    println!("{}", serde_json::to_string_pretty(&combined)?);

    Ok(())
}

/// Fetch node descriptions from the described endpoint.
async fn fetch_nodes(
    client: &reqwest::Client,
) -> Result<NymNodesDescribed, Box<dyn std::error::Error>> {
    let data = query_endpoint(client, &format!("{}/api/v1/nym-nodes/described", BASE_URL)).await?;
    let nodes: NymNodesDescribed = serde_json::from_value(data)?;
    Ok(nodes)
}

/// Loads all node families and builds a map: node_id → family name
async fn fetch_family_map(
    client: &reqwest::Client,
) -> Result<HashMap<u32, String>, Box<dyn std::error::Error>> {
    let url = format!("{}/api/v1/node-families", BASE_URL);
    let data = query_endpoint(client, &url).await?;
    let families: FamiliesResponse = serde_json::from_value(data)?;

    let mut map = HashMap::new();
    for family in families.data {
        for member in family.members {
            map.insert(member.node_id, family.name.clone());
        }
    }
    Ok(map)
}

fn node_roles(n: &Node) -> Vec<&'static str> {
    let mut roles = Vec::new();
    let r = &n.description.declared_role;

    if r.mixnode  { roles.push("Mixnode"); }
    if r.entry    { roles.push("Entry"); }
    if r.exit_ipr { roles.push("Exit-IPR"); }
    if r.exit_nr  { roles.push("Exit-NR"); }

    if n.description.network_requester.as_ref().map_or(false, |nr| !nr.address.is_empty()) {
        roles.push("Network-Requester");
    }
    if n.description.ip_packet_router.as_ref().map_or(false, |ipr| !ipr.address.is_empty()) {
        roles.push("IP-Packet-Router");
    }

    if roles.is_empty() { roles.push("-"); }
    roles
}

fn print_table(title: &str, headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
    }

    let gap = "  ";
    let line_len: usize = widths.iter().sum::<usize>() + gap.len() * (widths.len().saturating_sub(1));

    println!("\n{}", title);
    println!("{}", "=".repeat(line_len));

    let header_cells: Vec<String> = headers.iter().zip(&widths)
        .map(|(h, w)| format!("{:<width$}", h, width = *w))
        .collect();
    println!("{}", header_cells.join(gap));
    println!("{}", "-".repeat(line_len));

    for row in rows {
        let cells: Vec<String> = row.iter().zip(&widths)
            .map(|(c, w)| format!("{:<width$}", c, width = *w))
            .collect();
        println!("{}", cells.join(gap));
    }
    println!();
}

fn print_mixnodes(nodes: &[Node], family_map: &HashMap<u32, String>) {
    let mut items: Vec<&Node> = nodes.iter()
        .filter(|n| n.description.declared_role.mixnode)
        .collect();
    items.sort_by_key(|n| n.node_id);

    let rows: Vec<Vec<String>> = items.iter().map(|n| {
        let country = n.description.auxiliary_details.location
            .clone().unwrap_or_else(|| "??".to_string());
        let ip = if n.description.host_information.ip_address.is_empty() {
            "-".to_string()
        } else {
            n.description.host_information.ip_address.join(", ")
        };
        let host = n.description.host_information.hostname
            .clone().unwrap_or_else(|| "-".to_string());
        let family = family_map
            .get(&n.node_id)
            .cloned()
            .unwrap_or_else(|| "-".to_string());

        vec![n.node_id.to_string(), country, ip, host, family]
    }).collect();

    print_table(
        "MIXNODES",
        &["Node-ID", "Country", "IP Address", "Hostname", "Family"],
        &rows,
    );
}

fn print_gateways(nodes: &[Node], family_map: &HashMap<u32, String>) {
    let mut items: Vec<&Node> = nodes.iter()
        .filter(|n| {
            let r = &n.description.declared_role;
            r.entry || r.exit_ipr || r.exit_nr
        })
        .collect();
    items.sort_by_key(|n| n.node_id);

    let rows: Vec<Vec<String>> = items.iter().map(|n| {
        let country = n.description.auxiliary_details.location
            .clone().unwrap_or_else(|| "??".to_string());
        let ip = if n.description.host_information.ip_address.is_empty() {
            "-".to_string()
        } else {
            n.description.host_information.ip_address.join(", ")
        };
        let host = n.description.host_information.hostname
            .clone().unwrap_or_else(|| "-".to_string());
        let roles = node_roles(n).join(", ");
        let family = family_map
            .get(&n.node_id)
            .cloned()
            .unwrap_or_else(|| "-".to_string());

        vec![n.node_id.to_string(), country, ip, host, roles, family]
    }).collect();

    print_table(
        "GATEWAYS",
        &["Node-ID", "Country", "IP Address", "Hostname", "Roles", "Family"],
        &rows,
    );
}

fn print_network_requesters(nodes: &[Node], family_map: &HashMap<u32, String>) {
    let mut items: Vec<(&Node, &NetworkRequester)> = nodes.iter()
        .filter_map(|n| {
            let nr = n.description.network_requester.as_ref()?;
            if nr.address.is_empty() { return None; }
            Some((n, nr))
        })
        .collect();
    items.sort_by_key(|(n, _)| n.node_id);

    let rows: Vec<Vec<String>> = items.iter().map(|(n, nr)| {
        let country = n.description.auxiliary_details.location
            .clone().unwrap_or_else(|| "??".to_string());
        let ip = if n.description.host_information.ip_address.is_empty() {
            "-".to_string()
        } else {
            n.description.host_information.ip_address.join(", ")
        };
        let family = family_map
            .get(&n.node_id)
            .cloned()
            .unwrap_or_else(|| "-".to_string());

        vec![
            n.node_id.to_string(),
            country,
            ip,
            family,
            nr.address.clone(),
        ]
    }).collect();

    print_table(
        "NETWORK REQUESTERS - Nym address for nym-socks5-client --provider",
        &["Node-ID", "Country", "IP Address", "Family", "Nym Address (--provider)"],
        &rows,
    );

    println!("Tip:  nym-socks5-client init --id <ID> --provider <Nym-Address>");
}

fn find_pagination_mut(v: &mut Value) -> Option<&mut Value> {
    match v {
        Value::Object(map) => {
            if map.contains_key("pagination") { return map.get_mut("pagination"); }
            for val in map.values_mut() {
                if let Some(p) = find_pagination_mut(val) { return Some(p); }
            }
            None
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                if let Some(p) = find_pagination_mut(val) { return Some(p); }
            }
            None
        }
        _ => None,
    }
}

fn find_data_array_mut(v: &mut Value) -> Option<&mut Value> {
    match v {
        Value::Object(map) => {
            if let Some(Value::Array(_)) = map.get("data") { return map.get_mut("data"); }
            for val in map.values_mut() {
                if let Some(d) = find_data_array_mut(val) { return Some(d); }
            }
            None
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                if let Some(d) = find_data_array_mut(val) { return Some(d); }
            }
            None
        }
        _ => None,
    }
}

async fn query_endpoint(client: &reqwest::Client, url: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut page = 0;
    let mut combined_data = Vec::new();
    let mut base_json: Option<Value> = None;

    loop {
        let separator = if url.contains('?') { "&" } else { "?" };
        let page_url = format!("{}{}page={}&size=500", url, separator, page);
        let response = client.get(&page_url).send().await?;

        if !response.status().is_success() {
            if page == 0 {
                return Err(format!("HTTP {}", response.status()).into());
            }
            break;
        }

        let mut data: Value = response.json().await?;

        if page == 0 {
            base_json = Some(data.clone());
        }

        let has_pagination = find_pagination_mut(&mut data).is_some();

        if let Some(data_array) = find_data_array_mut(&mut data) {
            if let Some(arr) = data_array.as_array() {
                if arr.is_empty() && page > 0 {
                    break;
                }
                combined_data.extend(arr.clone());
            }
        } else if page > 0 {
            break;
        }

        if !has_pagination {
            if page == 0 {
                return Ok(data);
            }
            break;
        }

        if let Some(pag) = find_pagination_mut(&mut data) {
            if let (Some(total), Some(size)) = (
                pag.get("total").and_then(|v| v.as_u64()),
                pag.get("size").and_then(|v| v.as_u64()),
            ) {
                if size > 0 && (page + 1) * size >= total {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }

        page += 1;
    }

    if let Some(mut final_json) = base_json {
        if !combined_data.is_empty() {
            let data_len = combined_data.len();
            if let Some(data_array) = find_data_array_mut(&mut final_json) {
                *data_array = Value::Array(combined_data);
            }
            if let Some(pag) = find_pagination_mut(&mut final_json) {
                if let Some(obj) = pag.as_object_mut() {
                    obj.insert("total".to_string(), Value::Number(data_len.into()));
                    obj.insert("size".to_string(), Value::Number(data_len.into()));
                }
            }
        }
        Ok(final_json)
    } else {
        Err("No data fetched".into())
    }
}
