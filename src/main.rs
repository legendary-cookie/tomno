#![allow(dead_code)]

use std::{fs::File, io::Read};

use serde_derive::Deserialize;

use hcl::{Attribute, Block, Body};
use toml::Value;

#[derive(Debug, Deserialize)]
struct JobConfig {
    general: General,
    ports: Vec<Port>,
    services: Vec<Service>,
    containers: Vec<Container>,
    volumes: Vec<Volume>,
}

#[derive(Debug, Deserialize)]
struct General {
    name: String,
    count: u16,
    datacenters: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Port {
    name: String,
    to: u16,
}

#[derive(Debug, Deserialize)]
struct Service {
    name: String,
    port: String,
    tags: Vec<String>,
    check: ServiceCheck,
}

fn default_interval() -> String {
    "15s".to_string()
}

fn default_timeout() -> String {
    "3s".to_string()
}

fn default_false() -> bool {
    false
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct ServiceCheck {
    #[serde(rename = "type")]
    check_type: String,
    #[serde(default = "default_interval")]
    interval: String,
    #[serde(default = "default_timeout")]
    timeout: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct Container {
    name: String,
    image: String,
    ports: Vec<String>,
    mounts: Vec<ContainerMount>,
    env: Vec<EnvEntry>,
}

#[derive(Debug, Deserialize)]
struct EnvEntry {
    name: String,
    val: Value,
}

#[derive(Debug, Deserialize)]
struct ContainerMount {
    volume: String,
    mountpoint: String,
}

#[derive(Debug, Deserialize)]
struct Volume {
    name: String,
    #[serde(rename = "accessMode")]
    access_mode: String,
    #[serde(rename = "readOnly")]
    #[serde(default = "default_false")]
    read_only: bool,
}

fn get_vol_access_mode(s: &str) -> String {
    match s {
        "mnmw" => "multi-node-multi-writer".into(),
        "mnsw" => "multi-node-single-writer".into(),
        _ => panic!("Error parsing!"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        println!("Usage: nodock <path>");
        return Ok(());
    }

    let mut f = File::open(&args[1]).expect("no file found");
    let metadata = std::fs::metadata(&args[1]).expect("unable to read metadata");
    let mut buffer = vec![0; metadata.len() as usize];
    f.read(&mut buffer).expect("buffer overflow");

    let job: JobConfig = toml::from_str(String::from_utf8(buffer)?.as_str())?;

    let mut port_blocks: Vec<Block> = vec![];

    for port in job.ports {
        port_blocks.push(
            Block::builder("port")
                .add_label(port.name)
                .add_attribute(("to", port.to))
                .build(),
        );
    }

    let mut service_blocks: Vec<Block> = vec![];

    for svc in job.services {
        service_blocks.push(
            Block::builder("service")
                .add_attribute(("name", svc.name))
                .add_attribute(("port", svc.port))
                .add_attribute(("tags", svc.tags))
                .add_block(
                    Block::builder("check")
                        .add_attribute(("type", svc.check.check_type))
                        .add_attribute(("path", svc.check.path))
                        .add_attribute(("name", "app_health"))
                        .add_attribute(("interval", svc.check.interval))
                        .add_attribute(("timeout", svc.check.timeout))
                        .build(),
                )
                .build(),
        )
    }

    let mut volume_blocks: Vec<Block> = vec![];
    for vol in job.volumes {
        volume_blocks.push(
            Block::builder("volume")
                .add_label(&vol.name)
                .add_attribute(("type", "csi"))
                .add_attribute(("source", vol.name))
                .add_attribute(("access_mode", get_vol_access_mode(&vol.access_mode)))
                .add_attribute(("read_only", vol.read_only))
                .add_attribute(("attachment_mode", "filesystem"))
                .build(),
        )
    }

    let mut task_blocks: Vec<Block> = vec![];
    for container in job.containers {
        let mut mounts_blocks = vec![];
        for vol in container.mounts {
            mounts_blocks.push(
                Block::builder("volume_mount")
                    .add_attribute(("volume", vol.volume))
                    .add_attribute(("destination", vol.mountpoint))
                    .build(),
            )
        }
        let mut env_block_attributes = vec![];
        for e in container.env {
            if e.val.is_integer() {
                env_block_attributes.push(Attribute::new(e.name, e.val.as_integer().unwrap()))
            } else if e.val.is_str() {
                env_block_attributes.push(Attribute::new(e.name, e.val.as_str().unwrap()))
            }
        }

        task_blocks.push(
            Block::builder("task")
                .add_label(container.name)
                .add_attribute(("driver", "docker"))
                .add_block(
                    Block::builder("config")
                        .add_attribute(("image", container.image))
                        .add_attribute(("ports", container.ports))
                        .build(),
                )
                .add_blocks(mounts_blocks.into_iter())
                .add_block(
                    Block::builder("restart")
                        .add_attribute(("attempts", 3))
                        .add_attribute(("delay", "20s"))
                        .build(),
                )
                .add_block(
                    Block::builder("env")
                        .add_attributes(env_block_attributes.into_iter())
                        .build(),
                )
                .build(),
        )
    }

    let body = Body::builder()
        .add_block(
            Block::builder("job")
                .add_label(&job.general.name)
                .add_attribute(("datacenters", job.general.datacenters))
                .add_block(
                    Block::builder("group")
                        .add_label(&job.general.name)
                        .add_attribute(("count", job.general.count))
                        .add_block(
                            Block::builder("network")
                                .add_blocks(port_blocks.into_iter())
                                .build(),
                        )
                        .add_blocks(service_blocks.into_iter())
                        .add_blocks(volume_blocks.into_iter())
                        .add_blocks(task_blocks.into_iter())
                        .build(),
                )
                .build(),
        )
        .build();
    println!("{}", hcl::to_string(&body)?);
    Ok(())
}
