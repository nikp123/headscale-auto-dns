use reqwest;
use reqwest::{Url,header};
use clap::Parser;

use serde::Deserialize;
use serde_json::from_str;
//use serde_aux::field_attributes::deserialize_number_from_string;

use anyhow::Result;

#[derive(Parser)]
struct HeadscaleClientDetails {
    // https://github.com/juanfont/headscale/blob/109989005d414240bbe730ae1d8688dfe90d7e34/config-example.yaml#L33
    #[arg(long = "headscale_domain", alias = "hs_d", env = "HEADSCALE_DOMAIN",
        help = "Domain of Headscale server", default_value = "https://localhost:50433")]
    host: String,
    #[arg(long = "headscale_auth", alias = "hs_a", env = "HEADSCALE_AUTH",
        help = "Headscale API key")]
    auth: String,

    #[arg(long = "headscale_tld", alias = "hs_t", env = "HEADSCALE_TLD",
        help = r#"Headscale server's magicDNS's root level TLD (eg. something."tailscale")"#,
        default_values_t = vec!["tailscale".to_string()], value_delimiter = ',')]
    magic_tld: Vec<String>,
}

impl HeadscaleClientDetails {
    // We will skip validation in the config parser stage as we cannot do that
    // without setting up the client first
    fn new() -> Result<HeadscaleClientDetails> {
        Ok(HeadscaleClientDetails::parse())
    }
}

#[derive(Clone)]
pub struct HeadscaleClient {
    client: reqwest::blocking::Client,
    base_url: Url,

    // We need this in here because Headscale offers no API to access this information
    // as far as I've noticed
    magic_tld: Vec<String>,
}

impl HeadscaleClient {
    pub fn get_magic_tld(&self) -> Vec<String> {
        self.magic_tld.clone()
    }

    fn from(details: HeadscaleClientDetails) -> Result<HeadscaleClient> {
        let mut headers = reqwest::header::HeaderMap::new();

        let mut auth_value = header::HeaderValue::from_str(&format!("Bearer {}", &details.auth))?;
        auth_value.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, auth_value);

        let client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .build()?;

        let base_url = Url::parse(&details.host.to_string())?;

        Ok(HeadscaleClient {
            client,
            base_url,
            magic_tld: details.magic_tld,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let test_url = Url::parse(
            &(self.base_url.to_string() + "/api/v1/apikey"))?;

        let _res = self.client.get(test_url).send()?.error_for_status()?;

        Ok(())
    }

    pub fn new() -> Result<HeadscaleClient> {
        let client = HeadscaleClient::from(HeadscaleClientDetails::new()?)?;

        client.validate()?;

        Ok(client)
    }

    pub fn get_user_list(&self) -> Result<Vec<HeadscaleUser>> {
        let url = Url::parse(
            &(self.base_url.to_string() + "/api/v1/user"))?;

        let res = self.client.get(url).send()?.error_for_status()?;

        // Headscale API dumb asf so this had to do
        #[derive(Deserialize, Debug)]
        struct UserResponse {
            users: Vec<HeadscaleUser>,
        }

        Ok(from_str::<UserResponse>(&res.text()?)?.users)
    }
    pub fn get_node_list_with_addresses(&self) -> Result<Vec<HeadscaleNode>> {
        let urls = vec![Url::parse(&(self.base_url.to_string() + "/api/v1/node"))?];
        #[derive(Deserialize, Debug)]
        struct NodeResponse {
            nodes: Vec<HeadscaleNode>
        }

        let nodes: Vec<HeadscaleNode> = urls.into_iter().map(
            |url| {
                let res = self.client.get(url).send()
                    .unwrap()
                    .error_for_status()
                    .unwrap();

                from_str::<NodeResponse>(&res.text().unwrap()).unwrap().nodes
            }
        ).collect::<Vec<_>>().concat();

        Ok(nodes)
    }
}


#[derive(PartialEq, Deserialize, Debug, Clone)]
pub struct HeadscaleUser {
    //#[serde(deserialize_with = "deserialize_number_from_string")]
    //id:   u32,
    pub(crate) name: String,
}

pub fn headscale_user_list_contains_a_user(list: &Vec<HeadscaleUser>, user: &str) -> bool {
    for i in list { if i.name == user { return true; } }
    false
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HeadscaleNode {
    //#[serde(deserialize_with = "deserialize_number_from_string")]
    //pub id:           u32,
    pub ip_addresses: Vec<String>,   // CIDR notation
    //pub name:         String,      // node's own hostname
    pub given_name:   String,        // magicDNS machine name
    pub user:         HeadscaleUser,
    pub online:       bool,
}

impl HeadscaleNode {
    pub fn get_magic_dns_domains(&self, client: &HeadscaleClient) -> Vec<String> {
        client.get_magic_tld().into_iter()
            .map(|x| 
                self.given_name.clone() + "." + self.user.name.as_str() + "." + x.as_str())
            .collect()
    }
}
