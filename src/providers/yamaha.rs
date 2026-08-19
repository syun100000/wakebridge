use super::{ConnectionCheck, WakeProvider, WakeResult};
use crate::db::{DeviceRecord, SiteRecord};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use regex::Regex;
use ssh2::{HashType, MethodType, Session};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

const SSH_TIMEOUT: Duration = Duration::from_secs(12);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Clone, Default)]
pub struct YamahaRtxProvider;

#[async_trait]
impl WakeProvider for YamahaRtxProvider {
    async fn test_connection(
        &self,
        site: SiteRecord,
        credential: String,
    ) -> Result<ConnectionCheck> {
        tokio::task::spawn_blocking(move || {
            let command_result = connect_and_run(&site, &credential, "show version", false)?;
            Ok(ConnectionCheck {
                fingerprint: command_result.fingerprint,
                detail: summarize_router_output(&command_result.output),
            })
        })
        .await
        .context("join Yamaha connection test")?
    }

    async fn wake(
        &self,
        site: SiteRecord,
        device: DeviceRecord,
        credential: String,
    ) -> Result<WakeResult> {
        tokio::task::spawn_blocking(move || {
            let expected_fingerprint = site
                .ssh_host_key_fingerprint
                .as_deref()
                .context("SSH host key is not trusted for this site")?;
            let command = build_wol_command(
                &site.lan_interface,
                &device.mac_address,
                device.ip_address.as_deref(),
            )?;
            let command_result = connect_and_run(&site, &credential, &command, true)
                .with_context(|| format!("send WOL through site {}", site.name))?;
            if command_result.fingerprint != expected_fingerprint {
                bail!("SSH host key fingerprint changed");
            }
            Ok(WakeResult {
                detail: format!(
                    "Yamaha RTX accepted the fixed wol send command for {}",
                    device.mac_address
                ),
            })
        })
        .await
        .context("join Yamaha WOL task")?
    }
}

struct CommandResult {
    fingerprint: String,
    output: String,
}

fn connect_and_run(
    site: &SiteRecord,
    credential: &str,
    command: &str,
    require_trusted_key: bool,
) -> Result<CommandResult> {
    validate_router_host(&site.router_host)?;
    validate_ssh_username(&site.ssh_username)?;
    if site.provider != "yamaha_rtx" {
        bail!("unsupported provider");
    }
    if site.ssh_port == 0 {
        bail!("SSH port is invalid");
    }

    let address = (site.router_host.as_str(), site.ssh_port)
        .to_socket_addrs()
        .context("resolve router host")?
        .next()
        .context("router host did not resolve")?;
    let tcp = TcpStream::connect_timeout(&address, SSH_TIMEOUT)
        .with_context(|| format!("connect to {}:{}", site.router_host, site.ssh_port))?;
    tcp.set_read_timeout(Some(SSH_TIMEOUT))
        .context("set SSH read timeout")?;
    tcp.set_write_timeout(Some(SSH_TIMEOUT))
        .context("set SSH write timeout")?;

    let mut session = Session::new().context("create SSH session")?;
    session.set_tcp_stream(tcp);
    session.set_timeout(SSH_TIMEOUT.as_millis() as u32);
    if site.allow_legacy_ssh {
        enable_site_scoped_legacy_algorithms(&mut session)?;
    }
    session.handshake().context("SSH handshake")?;

    let fingerprint = session
        .host_key_hash(HashType::Sha256)
        .map(|hash| format!("SHA256:{}", STANDARD_NO_PAD.encode(hash)))
        .context("router did not provide an SSH host key")?;
    if let Some(expected) = site.ssh_host_key_fingerprint.as_deref() {
        if expected != fingerprint {
            bail!("SSH host key fingerprint mismatch");
        }
    } else if require_trusted_key {
        bail!("SSH host key is not trusted for this site");
    }

    session
        .userauth_password(&site.ssh_username, credential)
        .context("SSH password authentication")?;
    if !session.authenticated() {
        bail!("SSH authentication was rejected");
    }
    let output = run_interactive_command(&session, command)?;
    Ok(CommandResult {
        fingerprint,
        output,
    })
}

fn run_interactive_command(session: &Session, command: &str) -> Result<String> {
    if !is_safe_command(command) {
        bail!("internal command validation failed");
    }
    let mut channel = session.channel_session().context("open SSH channel")?;
    let _ = channel.request_pty("vt100", None, Some((120, 40, 0, 0)));
    channel.shell().context("open Yamaha SSH shell")?;
    session.set_blocking(false);

    let drain_deadline = Instant::now() + Duration::from_millis(500);
    let mut output = String::new();
    while Instant::now() < drain_deadline {
        read_available(&mut channel, &mut output)?;
        thread::sleep(Duration::from_millis(25));
    }

    channel
        .write_all(command.as_bytes())
        .context("write fixed Yamaha command")?;
    channel
        .write_all(b"\r")
        .context("terminate Yamaha command")?;
    channel.flush().context("flush Yamaha command")?;

    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        let eof = read_available(&mut channel, &mut output)?;
        if has_prompt(&output) || eof {
            break;
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for Yamaha command prompt");
        }
        thread::sleep(Duration::from_millis(40));
    }

    let lower = output.to_ascii_lowercase();
    if lower.contains("invalid")
        || lower.contains("unknown command")
        || lower.contains("command error")
        || lower.contains("error:")
    {
        bail!("Yamaha rejected the fixed command");
    }
    let _ = channel.close();
    let _ = channel.wait_close();
    Ok(output)
}

fn read_available(channel: &mut ssh2::Channel, output: &mut String) -> Result<bool> {
    let mut buffer = [0_u8; 4096];
    loop {
        match channel.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(size) => output.push_str(&String::from_utf8_lossy(&buffer[..size])),
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error).context("read Yamaha SSH output"),
        }
    }
}

fn has_prompt(output: &str) -> bool {
    output
        .lines()
        .rev()
        .map(str::trim_end)
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| {
            let trimmed = line.trim();
            trimmed.ends_with('>') || trimmed.ends_with('#')
        })
}

fn summarize_router_output(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn is_safe_command(command: &str) -> bool {
    !command.is_empty()
        && command.len() <= 256
        && !command.contains(['\r', '\n', ';', '|', '&', '\u{60}', '$'])
}

fn build_wol_command(interface: &str, mac: &str, ip_address: Option<&str>) -> Result<String> {
    let interface = validate_interface(interface)?;
    let mac = normalize_mac(mac)?;
    let mut command = format!("wol send -i 1 -c 3 {interface} {mac}");
    if let Some(ip_address) = ip_address {
        let ip_address = normalize_ip(ip_address)?;
        command.push_str(&format!(" {ip_address} udp 9"));
    }
    if !is_safe_command(&command) {
        bail!("internal WOL command validation failed");
    }
    Ok(command)
}

pub fn normalize_mac(value: &str) -> Result<String> {
    static MAC_RE: OnceLock<Regex> = OnceLock::new();
    let value = value.trim();
    let compact = if value.len() == 17 {
        let regex = MAC_RE.get_or_init(|| {
            Regex::new(r"(?i)^[0-9a-f]{2}([:-])[0-9a-f]{2}([:-])[0-9a-f]{2}([:-])[0-9a-f]{2}([:-])[0-9a-f]{2}([:-])[0-9a-f]{2}$")
                .expect("MAC regex")
        });
        if !regex.is_match(value) {
            bail!("MAC address must use six hexadecimal octets");
        }
        value.replace([':', '-'], "")
    } else if value.len() == 12 && value.chars().all(|character| character.is_ascii_hexdigit()) {
        value.to_owned()
    } else {
        bail!("MAC address must contain exactly six hexadecimal octets");
    };
    let bytes = (0..6)
        .map(|index| u8::from_str_radix(&compact[index * 2..index * 2 + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse MAC address")?;
    if bytes.iter().all(|byte| *byte == 0) {
        bail!("MAC address cannot be all zero");
    }
    if bytes[0] & 1 != 0 {
        bail!("multicast MAC addresses are not valid WOL targets");
    }
    Ok(bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":"))
}

pub fn normalize_ip(value: &str) -> Result<String> {
    let ip = value
        .trim()
        .parse::<std::net::Ipv4Addr>()
        .context("IP address must be a valid IPv4 address")?;
    if ip.is_multicast() || ip.is_unspecified() {
        bail!("IP address cannot be multicast or unspecified");
    }
    Ok(ip.to_string())
}

pub fn validate_fingerprint(value: &str) -> Result<String> {
    let value = value.trim();
    let encoded = value
        .strip_prefix("SHA256:")
        .context("SSH fingerprint must use SHA256:<base64> format")?;
    let decoded = STANDARD_NO_PAD
        .decode(encoded)
        .or_else(|_| STANDARD.decode(encoded))
        .context("invalid SSH fingerprint encoding")?;
    if decoded.len() != 32 {
        bail!("SSH SHA-256 fingerprint must contain 32 decoded bytes");
    }
    Ok(format!("SHA256:{}", STANDARD_NO_PAD.encode(decoded)))
}

fn validate_router_host(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 253
        || value.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, ';' | '|' | '&' | '\u{60}' | '$')
        })
    {
        bail!("router host contains invalid characters");
    }
    Ok(())
}

fn validate_ssh_username(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(|character| {
            character.is_control() || character.is_whitespace() || character == ':'
        })
    {
        bail!("SSH username contains invalid characters");
    }
    Ok(())
}

fn validate_interface(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 32
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-' | '/')
        })
    {
        bail!("LAN interface contains invalid characters");
    }
    Ok(value.to_owned())
}

fn enable_site_scoped_legacy_algorithms(session: &mut Session) -> Result<()> {
    session
        .method_pref(
            MethodType::Kex,
            "diffie-hellman-group14-sha1,diffie-hellman-group1-sha1",
        )
        .context("enable site-scoped legacy SSH KEX")?;
    session
        .method_pref(MethodType::HostKey, "ssh-rsa,ssh-dss")
        .context("enable site-scoped legacy SSH host keys")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_rejects_mac_addresses() {
        assert_eq!(
            normalize_mac("4c-cc-6a-b3-69-3e").expect("valid MAC"),
            "02:00:00:00:00:10"
        );
        assert!(normalize_mac("01:00:00:00:00:01").is_err());
        assert!(normalize_mac("00:00:00:00:00:00").is_err());
        assert!(normalize_mac("bad").is_err());
    }

    #[test]
    fn validates_ip_and_fingerprint() {
        assert_eq!(normalize_ip("192.0.2.10").expect("valid IP"), "192.0.2.10");
        assert!(normalize_ip("0.0.0.0").is_err());
        let encoded = STANDARD_NO_PAD.encode([7_u8; 32]);
        assert_eq!(
            validate_fingerprint(&format!("SHA256:{encoded}")).expect("valid fingerprint"),
            format!("SHA256:{encoded}")
        );
    }

    #[test]
    fn only_builds_fixed_wol_commands() {
        let command = build_wol_command("lan1", "02:00:00:00:00:10", Some("192.0.2.10"))
            .expect("fixed command");
        assert_eq!(
            command,
            "wol send -i 1 -c 3 lan1 02:00:00:00:00:10 192.0.2.10 udp 9"
        );
        assert!(build_wol_command("lan1;show version", "02:00:00:00:00:10", None).is_err());
    }
}
