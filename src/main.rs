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
    #[arg(short = 'm', long, conflicts_with_all = &["gateways", "network_requesters", "families", "all"])]
    mixnodes: bool,

    /// Gateways: Node-ID, Country, IP, Hostname, Roles, Family
    #[arg(short = 'g', long, conflicts_with_all = &["mixnodes", "network_requesters", "families", "all"])]
    gateways: bool,

    /// Network Requesters: Node-ID, Country, Nym Address
    #[arg(short = 'n', long, conflicts_with_all = &["mixnodes", "gateways", "families", "all", "geo"])]
    network_requesters: bool,

    /// Families: Family, Node-ID, Country, IPv4, Role (gateway/mixnode)
    #[arg(short = 'f', long, conflicts_with_all = &["mixnodes", "gateways", "network_requesters", "all"])]
    families: bool,

    /// All nodes: Family/Name, Node-ID, Country, IPv4, Role (gateway/mixnode)
    #[arg(short = 'a', long, conflicts_with_all = &["mixnodes", "gateways", "network_requesters", "families"])]
    all: bool,

    /// With -m/-g/-f/-a: also lookup and show Latitude / Longitude for each IPv4 (via ip-api.com)
    #[arg(long)]
    geo: bool,
}

const BASE_URL: &str = "https://validator.nymtech.net";

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

#[derive(Deserialize, Clone)]
struct FamiliesResponse {
    data: Vec<NodeFamily>,
}

#[derive(Deserialize, Clone)]
struct NodeFamily {
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
        print_mixnodes(&nodes.data, &family_map, cli.geo, &client).await?;
        return Ok(());
    }

    if cli.gateways {
        let (nodes, family_map) = tokio::try_join!(
            fetch_nodes(&client),
            fetch_family_map(&client)
        )?;
        print_gateways(&nodes.data, &family_map, cli.geo, &client).await?;
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

    if cli.families {
        let (nodes, family_map) = tokio::try_join!(
            fetch_nodes(&client),
            fetch_family_map(&client)
        )?;
        print_families(&client, &nodes.data, &family_map, cli.geo).await?;
        return Ok(());
    }

    if cli.all {
        let (nodes, family_map) = tokio::try_join!(
            fetch_nodes(&client),
            fetch_family_map(&client)
        )?;
        print_all(&client, &nodes.data, &family_map, cli.geo).await?;
        return Ok(());
    }

    eprintln!("No view selected. Use -m, -g, -n, -f, or -a");
    Ok(())
}

async fn fetch_nodes(
    client: &reqwest::Client,
) -> Result<NymNodesDescribed, Box<dyn std::error::Error>> {
    let data = query_endpoint(client, &format!("{}/api/v1/nym-nodes/described", BASE_URL)).await?;
    let nodes: NymNodesDescribed = serde_json::from_value(data)?;
    Ok(nodes)
}

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
    if r.mixnode { roles.push("Mixnode"); }
    if r.entry { roles.push("Entry"); }
    if r.exit_ipr { roles.push("Exit-IPR"); }
    if r.exit_nr { roles.push("Exit-NR"); }
    if n.description.network_requester.as_ref().map_or(false, |nr| !nr.address.is_empty()) {
        roles.push("Network-Requester");
    }
    if n.description.ip_packet_router.as_ref().map_or(false, |ipr| !ipr.address.is_empty()) {
        roles.push("IP-Packet-Router");
    }
    if roles.is_empty() { roles.push("-"); }
    roles
}

fn print_table(title: &str, headers: &[&str], rows: &[Vec<String>], right_align: bool) {
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

    let header_cells: Vec<String> = headers.iter().enumerate().zip(&widths)
        .map(|((i, h), w)| {
            if right_align && i > 0 {
                format!("{:>width$}", h, width = *w)
            } else {
                format!("{:<width$}", h, width = *w)
            }
        })
        .collect();
    println!("{}", header_cells.join(gap));
    println!("{}", "-".repeat(line_len));

    for row in rows {
        let cells: Vec<String> = row.iter().enumerate().zip(&widths)
            .map(|((i, c), w)| {
                if right_align && i > 0 {
                    format!("{:>width$}", c, width = *w)
                } else {
                    format!("{:<width$}", c, width = *w)
                }
            })
            .collect();
        println!("{}", cells.join(gap));
    }
    println!();
}

async fn print_mixnodes(
    nodes: &[Node],
    family_map: &HashMap<u32, String>,
    with_geo: bool,
    client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut items: Vec<&Node> = nodes.iter()
        .filter(|n| n.description.declared_role.mixnode)
        .collect();

    items.sort_by_key(|n| n.node_id);

    let geo_map = if with_geo {
        let unique_ips: Vec<String> = items.iter()
            .flat_map(|n| {
                n.description.host_information.ip_address.iter()
                    .filter(|ip| is_ipv4(ip))
                    .cloned()
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        eprintln!("Looking up geo coordinates for {} unique IPv4 addresses...", unique_ips.len());
        fetch_geo_coords(client, &unique_ips).await?
    } else {
        HashMap::new()
    };

    let rows: Vec<Vec<String>> = items.iter().map(|n| {
        let country = n.description.auxiliary_details.location
            .clone()
            .map(|c| strip_emojis(&c))
            .unwrap_or_else(|| "??".to_string());
        let ip = if n.description.host_information.ip_address.is_empty() {
            "-".to_string()
        } else {
            n.description.host_information.ip_address.join(", ")
        };
        let host = n.description.host_information.hostname
            .clone()
            .map(|h| strip_emojis(&h))
            .unwrap_or_else(|| "-".to_string());
        let family = family_map
            .get(&n.node_id)
            .map(|f| strip_emojis(f))
            .unwrap_or_else(|| "-".to_string());

        let mut row = vec![
            n.node_id.to_string(),
            country,
            ip.clone(),
            host,
            family,
        ];

        if with_geo {
            let ipv4s: Vec<&str> = n.description.host_information.ip_address
                .iter()
                .filter(|ip| is_ipv4(ip))
                .map(|s| s.as_str())
                .collect();
            let (lat_s, lon_s) = if let Some(first_ip) = ipv4s.first() {
                if let Some(&(lat, lon)) = geo_map.get(*first_ip) {
                    (format!("{:.4}", lat), format!("{:.4}", lon))
                } else {
                    ("-".to_string(), "-".to_string())
                }
            } else {
                ("-".to_string(), "-".to_string())
            };
            row.push(lat_s);
            row.push(lon_s);
        }

        row
    }).collect();

    let headers: &[&str] = if with_geo {
        &["Node-ID", "Country", "IP Address", "Hostname", "Family", "Lat", "Lon"]
    } else {
        &["Node-ID", "Country", "IP Address", "Hostname", "Family"]
    };

    print_table("MIXNODES", headers, &rows, false);
    Ok(())
}

async fn print_gateways(
    nodes: &[Node],
    family_map: &HashMap<u32, String>,
    with_geo: bool,
    client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut items: Vec<&Node> = nodes.iter()
        .filter(|n| {
            let r = &n.description.declared_role;
            r.entry || r.exit_ipr || r.exit_nr
        })
        .collect();

    items.sort_by_key(|n| n.node_id);

    let geo_map = if with_geo {
        let unique_ips: Vec<String> = items.iter()
            .flat_map(|n| {
                n.description.host_information.ip_address.iter()
                    .filter(|ip| is_ipv4(ip))
                    .cloned()
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        eprintln!("Looking up geo coordinates for {} unique IPv4 addresses...", unique_ips.len());
        fetch_geo_coords(client, &unique_ips).await?
    } else {
        HashMap::new()
    };

    let rows: Vec<Vec<String>> = items.iter().map(|n| {
        let country = n.description.auxiliary_details.location
            .clone()
            .map(|c| strip_emojis(&c))
            .unwrap_or_else(|| "??".to_string());
        let ip = if n.description.host_information.ip_address.is_empty() {
            "-".to_string()
        } else {
            n.description.host_information.ip_address.join(", ")
        };
        let host = n.description.host_information.hostname
            .clone()
            .map(|h| strip_emojis(&h))
            .unwrap_or_else(|| "-".to_string());
        let roles = node_roles(n).join(", ");
        let family = family_map
            .get(&n.node_id)
            .map(|f| strip_emojis(f))
            .unwrap_or_else(|| "-".to_string());

        let mut row = vec![
            n.node_id.to_string(),
            country,
            ip.clone(),
            host,
            roles,
            family,
        ];

        if with_geo {
            let ipv4s: Vec<&str> = n.description.host_information.ip_address
                .iter()
                .filter(|ip| is_ipv4(ip))
                .map(|s| s.as_str())
                .collect();
            let (lat_s, lon_s) = if let Some(first_ip) = ipv4s.first() {
                if let Some(&(lat, lon)) = geo_map.get(*first_ip) {
                    (format!("{:.4}", lat), format!("{:.4}", lon))
                } else {
                    ("-".to_string(), "-".to_string())
                }
            } else {
                ("-".to_string(), "-".to_string())
            };
            row.push(lat_s);
            row.push(lon_s);
        }

        row
    }).collect();

    let headers: &[&str] = if with_geo {
        &["Node-ID", "Country", "IP Address", "Hostname", "Roles", "Family", "Lat", "Lon"]
    } else {
        &["Node-ID", "Country", "IP Address", "Hostname", "Roles", "Family"]
    };

    print_table("GATEWAYS", headers, &rows, false);
    Ok(())
}

fn print_network_requesters(nodes: &[Node], _family_map: &HashMap<u32, String>) {
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
            .clone()
            .map(|c| strip_emojis(&c))
            .unwrap_or_else(|| "??".to_string());

        vec![
            n.node_id.to_string(),
            country,
            nr.address.clone(),
        ]
    }).collect();

    print_table(
        "NETWORK REQUESTERS - Nym address for nym-socks5-client --provider",
        &["Node-ID", "Country", "Nym Address (--provider)"],
        &rows,
        false,
    );
    println!("Tip:  nym-socks5-client init --id <ID> --provider <Nym-Address>");
}

fn is_ipv4(ip: &str) -> bool {
    ip.contains('.') && !ip.contains(':')
}

fn strip_emojis(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            let u = c as u32;
            !(matches!(u,
                0x1F300..=0x1FAFF |
                0x2600..=0x27BF  |
                0x2300..=0x23FF  |
                0x2B00..=0x2BFF  |
                0xFE00..=0xFE0F  |
                0x1F000..=0x1F02F |
                0x1F0A0..=0x1F0FF |
                0xE0000..=0xE007F
            ) || c.is_control())
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Deserialize)]
struct IpApiResult {
    status: String,
    query: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
}

async fn fetch_geo_coords(
    client: &reqwest::Client,
    ips: &[String],
) -> Result<HashMap<String, (f64, f64)>, Box<dyn std::error::Error>> {
    let mut map = HashMap::new();
    if ips.is_empty() {
        return Ok(map);
    }

    const BATCH_SIZE: usize = 100;
    for (i, chunk) in ips.chunks(BATCH_SIZE).enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }
        eprintln!("  Geo lookup batch {}/{} ({} IPs)...", i + 1, (ips.len() + BATCH_SIZE - 1) / BATCH_SIZE, chunk.len());
        let body = serde_json::to_value(chunk)?;
        let resp = client
            .post("http://ip-api.com/batch?fields=status,query,lat,lon")
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            eprintln!("  Warning: geo batch HTTP {}", resp.status());
            continue;
        }
        let results: Vec<IpApiResult> = resp.json().await?;
        for r in results {
            if r.status == "success" {
                if let (Some(q), Some(lat), Some(lon)) = (r.query, r.lat, r.lon) {
                    map.insert(q, (lat, lon));
                }
            }
        }
    }
    Ok(map)
}

async fn print_families(
    client: &reqwest::Client,
    nodes: &[Node],
    family_map: &HashMap<u32, String>,
    with_geo: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut items: Vec<(&Node, &str, String)> = nodes.iter()
        .filter_map(|n| {
            let family_raw = family_map.get(&n.node_id)?;
            let r = &n.description.declared_role;
            let is_mix = r.mixnode;
            let is_gw = r.entry || r.exit_ipr || r.exit_nr;
            if !is_mix && !is_gw {
                return None;
            }
            let role = if is_mix { "mixnode" } else { "gateway" };
            let family_clean = strip_emojis(family_raw);
            Some((n, role, family_clean))
        })
        .collect();

    items.sort_by(|a, b| {
        a.2.cmp(&b.2).then_with(|| a.0.node_id.cmp(&b.0.node_id))
    });

    let geo_map = if with_geo {
        let mut unique_ips: Vec<String> = items.iter()
            .flat_map(|(n, _, _)| {
                n.description.host_information.ip_address.iter()
                    .filter(|ip| is_ipv4(ip))
                    .cloned()
            })
            .collect();
        unique_ips.sort();
        unique_ips.dedup();
        eprintln!("Looking up geo coordinates for {} unique IPv4 addresses...", unique_ips.len());
        fetch_geo_coords(client, &unique_ips).await?
    } else {
        HashMap::new()
    };

    let rows: Vec<Vec<String>> = items.iter().map(|(n, role, family)| {
        let country = n.description.auxiliary_details.location
            .clone()
            .map(|c| strip_emojis(&c))
            .unwrap_or_else(|| "??".to_string());
        let ipv4s: Vec<&str> = n.description.host_information.ip_address
            .iter()
            .filter(|ip| is_ipv4(ip))
            .map(|s| s.as_str())
            .collect();
        let ip = if ipv4s.is_empty() {
            "-".to_string()
        } else {
            ipv4s.join(", ")
        };

        let mut row = vec![
            family.clone(),
            n.node_id.to_string(),
            country,
            ip.clone(),
        ];

        if with_geo {
            let (lat_s, lon_s) = if let Some(first_ip) = ipv4s.first() {
                if let Some(&(lat, lon)) = geo_map.get(*first_ip) {
                    (format!("{:.4}", lat), format!("{:.4}", lon))
                } else {
                    ("-".to_string(), "-".to_string())
                }
            } else {
                ("-".to_string(), "-".to_string())
            };
            row.push(lat_s);
            row.push(lon_s);
        }

        row.push(role.to_string());
        row
    }).collect();

    let headers: &[&str] = if with_geo {
        &["Family", "Node-ID", "Country", "IP Address (v4)", "Lat", "Lon", "Role"]
    } else {
        &["Family", "Node-ID", "Country", "IP Address (v4)", "Role"]
    };

    print_table("FAMILIES", headers, &rows, true);
    Ok(())
}

async fn print_all(
    client: &reqwest::Client,
    nodes: &[Node],
    family_map: &HashMap<u32, String>,
    with_geo: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut items: Vec<(&Node, String, &str)> = nodes.iter()
        .filter_map(|n| {
            let r = &n.description.declared_role;
            let is_mix = r.mixnode;
            let is_gw = r.entry || r.exit_ipr || r.exit_nr;
            if !is_mix && !is_gw {
                return None;
            }
            let role = if is_mix { "mixnode" } else { "gateway" };
            let family = family_map
                .get(&n.node_id)
                .map(|f| strip_emojis(f))
                .unwrap_or_else(|| "-".to_string());
            Some((n, family, role))
        })
        .collect();

    items.sort_by(|a, b| {
        a.1.cmp(&b.1).then_with(|| a.0.node_id.cmp(&b.0.node_id))
    });

    let geo_map = if with_geo {
        let unique_ips: Vec<String> = items.iter()
            .flat_map(|(n, _, _)| {
                n.description.host_information.ip_address.iter()
                    .filter(|ip| is_ipv4(ip))
                    .cloned()
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        eprintln!("Looking up geo coordinates for {} unique IPv4 addresses...", unique_ips.len());
        fetch_geo_coords(client, &unique_ips).await?
    } else {
        HashMap::new()
    };

    let rows: Vec<Vec<String>> = items.iter().map(|(n, family, role)| {
        let country = n.description.auxiliary_details.location
            .clone()
            .map(|c| strip_emojis(&c))
            .unwrap_or_else(|| "??".to_string());
        let ipv4s: Vec<&str> = n.description.host_information.ip_address
            .iter()
            .filter(|ip| is_ipv4(ip))
            .map(|s| s.as_str())
            .collect();
        let ip = if ipv4s.is_empty() {
            "-".to_string()
        } else {
            ipv4s.join(", ")
        };

        let mut row = vec![
            family.clone(),
            n.node_id.to_string(),
            country,
            ip.clone(),
            role.to_string(),
        ];

        if with_geo {
            let (lat_s, lon_s) = if let Some(first_ip) = ipv4s.first() {
                if let Some(&(lat, lon)) = geo_map.get(*first_ip) {
                    (format!("{:.4}", lat), format!("{:.4}", lon))
                } else {
                    ("-".to_string(), "-".to_string())
                }
            } else {
                ("-".to_string(), "-".to_string())
            };
            row.push(lat_s);
            row.push(lon_s);
        }

        row
    }).collect();

    let headers: &[&str] = if with_geo {
        &["Family/Name", "Node-ID", "Country", "IP (v4)", "Role", "Lat", "Lon"]
    } else {
        &["Family/Name", "Node-ID", "Country", "IP (v4)", "Role"]
    };

    print_table("ALL NODES", headers, &rows, true);
    Ok(())
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
