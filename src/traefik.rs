use std::string::ToString;
use base64::Engine;
use clap::Parser;
use base64::prelude::BASE64_STANDARD;
use reqwest::{header, Url};
use serde::Deserialize;
use serde_json::from_str;
use thiserror::Error;
use regex::Regex;

#[derive(Debug, Error)]
enum TraefikUserError {
    #[error(r#"No valid Traefik host found.\
Either no eligible Headscale servers exist or no Traefik hosts was defined."#)]
    NoHosts,
    #[error(r#"No valid Traefik URL prefix (eg. "https://") has been specified"#)]
    NoPrefix,
    #[error(r#"No valid Traefik URL suffix (eg. "/trafik") has been specified"#)]
    NoSuffix,
}

// Don't derive Debug as it can leak sensitive info the syslog
#[derive(Parser, Clone, Debug)]
pub struct TraefikAPIClientDetails {
    // In case of a double match in a query, preferred servers are picked first
    #[arg(long = "traefik_domain", alias = "td", env = "TRAEFIK_DOMAIN",
        help = "Domain of a single Traefik server (more can be added via Tailscale)")]
    host: Option<String>,
    #[arg(long = "traefik_domain_prefix", alias = "tdp", env = "TRAEFIK_DOMAIN_PREFIX",
        help = r#"Prefixes appended to generated Traefik server names \
(these names are obtained from your headscale server)"#)]
    prefix: Option<String>,
    #[arg(long = "traefik_domain_suffix", alias = "tds", env = "TRAEFIK_DOMAIN_SUFFIX",
        help = r#"Suffixes appended to generated Traefik server names \
(these names are obtained from your headscale server)"#)]
    suffix: Option<String>,
    #[arg(long = "traefik_user", alias = "tu", env = "TRAEFIK_USER",
        help = "Traefik basic authentication user (shared among all hosts)")]
    user: String,
    #[arg(long = "traefik_pass", alias = "tp", env = "TRAEFIK_PASS",
        help = "Traefik basic authentication password (shared among all hosts)")]
    password: String,
}

impl TraefikAPIClientDetails {
    fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.prefix.is_some() {
            return Err(Box::new(TraefikUserError::NoPrefix))
        }

        if !self.suffix.is_some() {
            return Err(Box::new(TraefikUserError::NoSuffix))
        }

        if !self.host.is_some() {
            return Err(Box::new(TraefikUserError::NoHosts))
        }

        Ok(())
    }
    pub fn from_custom_host(host: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut details = TraefikAPIClientDetails::parse();
        details.host = Some(host.to_string());

        details.validate()?;

        Ok(details)
    }

    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let details = TraefikAPIClientDetails::parse();

        details.validate()?;

        Ok(TraefikAPIClientDetails::parse())
    }
}

#[derive(Clone)]
pub struct TraefikAPIClient {
    base_url: Url,
    client: reqwest::blocking::Client,
}

// This API response is much fatter, but I don't need most of it
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TraefikRouter {
    //entry_points: Vec<String>,
    service:      String,
    rule:         String,
    //status:       String,

    // We use this field to determine if a certain middleware needs to be present
    // for the logic to decide whether to include it in the final DNS output
    pub middlewares: Option<Vec<String>>,
}

// Implement PartialEq manually
impl PartialEq for TraefikRouter {
    fn eq(&self, other: &Self) -> bool {
        // Compare only the desired fields
        self.service == other.service && self.rule == other.rule
    }
}

impl TraefikRouter {
    pub fn get_domain_list(&self) -> Vec<String> {
        // Regular expression to match domain names in Traefik rules
        let re = Regex::new(r#"(Host)\(`([a-z0-9,.]+)`\)"#).unwrap();
        let mut domains = Vec::new();

        for cap in re.captures_iter(self.rule.as_str()) {
            if let Some(domain) = cap.get(2) {
                domains.push(domain.as_str().to_string());
            }
        }

        domains
    }
}

impl TraefikAPIClient {
    pub fn from(details: &TraefikAPIClientDetails) -> Result<Self, Box<dyn std::error::Error>> {
        let mut headers = header::HeaderMap::new();

        let authorization: String = String::from(&details.user) + ":" + &details.password;
        let authorization = BASE64_STANDARD.encode(authorization);

        let mut auth_value = header::HeaderValue::from_str(&format!("Basic {}", authorization))?;
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);

        let client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()?;

        let url: String = String::from(&details.prefix.clone().unwrap()) +
            &details.host.clone().unwrap() + &details.suffix.clone().unwrap();

        let base_url = Url::parse(url.as_str())?;

        Ok(TraefikAPIClient {
            base_url,
            client
        })
    }

    #[allow(dead_code)]
    pub fn validate(client: &Self) -> Result<(), Box<dyn std::error::Error>> {
        let url = Url::parse(&(client.base_url.to_string() + "/api/overview"))?;

        let _res = client.client.get(url).send()?.error_for_status()?;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let details = TraefikAPIClientDetails::new()?;

        let client = TraefikAPIClient::from(&details)?;

        Self::validate(&client)?;

        Ok(client)
    }

    pub fn get_router_list(client: &Self) -> Result<Vec<TraefikRouter>, Box<dyn std::error::Error>> {
        let urls = Url::parse(&(client.base_url.to_string() + "/api/http/routers"))?;
        let res = client.client.get(urls).send()?.error_for_status()?;
        let routers = from_str::<Vec<TraefikRouter>>(&res.text()?)?;

        Ok(routers)
    }
}
