use std::fs::File;
use std::io::BufWriter;
use clap::Parser;
use dotenv::dotenv;
use serde::{Serialize};
use std::rc::Rc;
use anyhow::{Result, Context};
use regex::Regex;
use crate::headscale::{headscale_user_list_contains_a_user, HeadscaleClient, HeadscaleNode, HeadscaleUser};
use crate::traefik::{TraefikAPIClient, TraefikAPIClientDetails, TraefikRouter};

#[derive(Parser)]
struct ProcessingSetup {
    #[arg(long = "traefik_middleware_whitelist", alias = "tmw", env = "TRAEFIK_MIDDLEWARE_WHITELIST",
        help = r#"What middlewares (if none, no filtering happens) need to be present
in order for the domain to be added to the tailscale DNS"#, value_delimiter = ',',
        default_values_t = Vec::<String>::new())]
    middlewares: Vec<String>,

    #[arg(long = "headscale_allowed_users", alias = "hs_au", env = "HEADSCALE_ALLOWED_USERS",
        help = r#"Filter machines that are queried thru Traefik based on their Tailscale user
 (empty to allow all machines)"#, value_delimiter = ',', default_values_t = Vec::<String>::new())]
    allowed_users: Vec<String>,

    #[arg(long = "headscale_blacklisted_nodes", alias = "hs_bn", env = "HEADSCALE_BLACKLISTED_NODES",
        help = r#"Filter out machines for Traefik querying based on their Tailscale hostname
  (empty to allow all)"#, value_delimiter = ',', default_values_t = Vec::<String>::new())]
    node_blacklist: Vec<String>,

    #[arg(long = "domain_whitelist", alias = "dw", env = "DOMAIN_WHITELIST",
        help = r#"A whitelist regex which decides what domains to include in the final output
The whitelist is processed first."#)]
    domain_whitelist_regex: Option<String>,

    #[arg(long = "domain_blacklist", alias = "db", env = "DOMAIN_BLACKLIST",
        help = r#"A blacklist regex which decides what domains to exclude from the final output.
The blacklist is processed last."#)]
    domain_blacklist_regex: Option<String>,

    #[arg(long = "output", short = 'o', env = "OUTPUT",
        help = r#"Path where the generated json extra_records.json will be written to.
Make sure you configure Headscale to read from this path."#, default_value = "extra_records.json")]
    output_path: String,

    #[arg(long = "headscale_old_magicdns", alias = "hs_olddns", env = "HEADSCALE_OLD_MAGICDNS",
        help = r#"Provide old magicDNS functionality to Headscale,
ie. the old `node.user.base_domain` format"#, default_value_t = true)]
    old_magicdns: bool,
}

// values that are expected to change during runtime
struct ProcessingVolatile {
    // Caution! This includes all headscale nodes, including the ones not used by Treafik!
    // This is required for the old magicDNS functionality to do it's thing
    headscale_nodes: Vec<Rc<HeadscaleNode>>,
    // This ONLY includes users that are searched for when searching for Traefik endpoints,
    // may be changed in the future to behave in the same way as nodes does.
    headscale_users: Vec<HeadscaleUser>,
    traefik_clients: Vec<(TraefikAPIClient, Rc<HeadscaleNode>)>,
    traefik_router:  Vec<(TraefikRouter,    Rc<HeadscaleNode>)>,
}
// basic wrapper impl just to make rust behave
impl ProcessingVolatile {
    fn new() -> ProcessingVolatile {
        ProcessingVolatile {
            headscale_users: Vec::new(),
            headscale_nodes: Vec::new(),
            traefik_clients: Vec::new(),
            traefik_router:  Vec::new(),
        }
    }
}

// values placed here are meant to stay during the entire session
pub struct Processing {
    setup: ProcessingSetup,
    headscale_client: HeadscaleClient,
    domain_whitelist: Option<Regex>,
    domain_blacklist: Option<Regex>,
    volatile: ProcessingVolatile,
}

impl Processing {
    pub fn new() -> Result<Self> {
        // Load in the dotenv variables
        dotenv().ok();

        let setup = ProcessingSetup::parse();

        Ok(Self {
            headscale_client: HeadscaleClient::new()
                .context("Failed to initialize the Headscale client")?,
            volatile: ProcessingVolatile::new(),
            domain_whitelist: match &setup.domain_whitelist_regex {
                Some(r) => Some(Regex::new(r).context("The whitelist regex is invalid")?),
                None    => None,
            },
            domain_blacklist: match &setup.domain_blacklist_regex {
                Some(r) => Some(Regex::new(r).context("The blacklist regex is invalid")?),
                None    => None,
            },
            setup,
        })
    }

    pub fn update_servers(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.volatile.headscale_users = self.headscale_client.get_user_list()?;

        // if user filtering is enabled
        if self.setup.allowed_users.len() > 0 {
            self.volatile.headscale_users.retain(|x| self.setup.allowed_users.contains(&x.name));
        }

        // Get the list of all Headscale nodes
        let all_headscale_nodes = self.headscale_client.get_node_list_with_addresses()?;

        // Turn it into a reference counted list
        self.volatile.headscale_nodes = all_headscale_nodes.into_iter()
            .map(|x| Rc::new(x)).collect();

        // Create a second list that only contains a list of nodes that are
        // in the interest of Traefik only
        let mut traefik_only_node_list: Vec<Rc<HeadscaleNode>> = self.volatile.headscale_nodes.iter()
            .filter(|&x| headscale_user_list_contains_a_user(
                        &self.volatile.headscale_users, x.user.name.as_str()
            )).map(|x| Rc::clone(x)).collect();

        // Filter out any undesired nodes
        if self.setup.node_blacklist.len() > 0 {
            traefik_only_node_list.retain(|x| !self.setup.node_blacklist.contains(&x.given_name));
        }

        // Generate a list of Traefik clients using the the smaller list we just made
        self.volatile.traefik_clients = Vec::new();
        for i in traefik_only_node_list {
            let details = TraefikAPIClientDetails::from_custom_host(
                // either way, IPv4 or IPv6 would work here so we don't really care
                i.ip_addresses[0].as_str()
            )?;
            let client = TraefikAPIClient::from(&details)?;

            self.volatile.traefik_clients.push((client, Rc::clone(&i)));
        }

        Ok(())
    }

    pub fn update_routers(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut all_routers: Vec<(TraefikRouter, Rc<HeadscaleNode>)> = Vec::new();

        for (client, node) in &self.volatile.traefik_clients {
            let mut routers = TraefikAPIClient::get_router_list(&client)?;

            let existing_routers: Vec<&TraefikRouter> = all_routers.iter()
                .map(|(x, _)| x)
                .collect::<Vec<_>>();

            routers.retain(|router| !existing_routers.contains(&router));

            for i in routers {
                all_routers.push((i, Rc::clone(node)));
            }
        }

        self.volatile.traefik_router = all_routers;

        Ok(())
    }

    pub fn generate_json(&self) -> Result<(), Box<dyn std::error::Error>> {
        #[derive(Serialize, Debug)]
        struct HeadscaleDNSEntry {
            name:        String, // DNS domain we're trying to resolve
            #[serde(rename = "type")] // we can't name it "type" in rust as its a reserved keyword
            record_type: String, // Record type (A or AAAA only, for now)
            value:       String,
        }

        impl PartialEq for HeadscaleDNSEntry {
            fn eq(&self, other: &Self) -> bool {
                if (self.name == other.name) &&
                    (self.record_type == other.record_type) {
                    return true;
                }
                false
            }
        }

        let mut dns_entries: Vec<HeadscaleDNSEntry> = Vec::new();

        for (router, client) in &self.volatile.traefik_router {
            // drop dns entries based on whether a middleware exists or not
            let mut middleware_found = false;

            // loop through set up middlewares
            for i in &self.setup.middlewares {
                match router.middlewares {
                    Some(ref middlewares) => {
                        if middlewares.contains(&i) {
                            middleware_found = true;
                            break;
                        }
                    }
                    // skip if no middleware list exists
                    None => break,
                }
            }

            // skip if a middleware whitelist exists and a middleware has not been found
            if !middleware_found && !self.setup.middlewares.is_empty() { continue; }

            // get list domains associated with each traefik router
            let domains = router.get_domain_list();

            // skip rules that do not contain a domain
            if domains.is_empty() { continue; }

            for ip in &client.ip_addresses {
                for domain in &domains {
                    let dns_entry = HeadscaleDNSEntry {
                        record_type: (if ip.contains(':') { "AAAA" } else { "A" }).to_string(),
                        value: ip.clone(),
                        name: domain.clone(),
                    };

                    // we don't want duplicates
                    if dns_entries.contains(&dns_entry) {
                        continue;
                    }

                    match &self.domain_whitelist {
                        Some(r) => if !r.is_match(&dns_entry.name) { continue; },
                        None => {},
                    }

                    match &self.domain_blacklist {
                        Some(r) => if r.is_match(&dns_entry.name) { continue; },
                        None => {},
                    }

                    dns_entries.push(dns_entry);
                }
            }
        }
        
        // subroutine that adds the magicDNS domains
        if self.setup.old_magicdns {
            for i in &self.volatile.headscale_nodes {
                for j in &i.ip_addresses {
                    for k in i.get_magic_dns_domains(&self.headscale_client) {
                        dns_entries.push(HeadscaleDNSEntry{
                            name: k,
                            record_type: if j.contains(':') { "AAAA".to_string() } else { "A".to_string() },
                            value: j.clone(),
                        });
                    }
                }
            }
        }

        let file = File::create(&self.setup.output_path)
            .context(r#"Unable to write to the output file.
Make sure that the output path is correct!"#)?;

        let writer = BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &dns_entries)?;

        Ok(())
    }
}
