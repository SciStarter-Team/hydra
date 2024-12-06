use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::Error;
use async_nats::service::Request;
use common::{message::location::LocationInfo, shared::db};

async fn lookup_ipv4(addr: Ipv4Addr) -> Result<LocationInfo, Error> {
    let ipnumber = addr.to_bits();

    let info = sqlx::query_as!(
        LocationInfo,
        r#"select
             country_code as "country_code!",
             country_name as "country_name!",
             region_name as "region_name!",
             city_name as "city_name!",
             latitude as "latitude!",
             longitude as "longitude!",
             zip_code as "zip_code!",
             time_zone as "time_zone!"
           from ipv4location
           where ip_to >= ?1 and ip_from <= ?1
           order by ip_to limit 1
        "#,
        ipnumber
    )
    .fetch_one(db())
    .await?;

    Ok(info)
}

async fn lookup_ipv6(addr: Ipv6Addr) -> Result<LocationInfo, Error> {
    let ipnumber = addr.to_bits().to_string();

    let info = sqlx::query_as!(
        LocationInfo,
        r#"select
             country_code as "country_code!",
             country_name as "country_name!",
             region_name as "region_name!",
             city_name as "city_name!",
             latitude as "latitude!",
             longitude as "longitude!",
             zip_code as "zip_code!",
             time_zone as "time_zone!"
           from ipv6location
           where ip_to >= ?1 and ip_from <= ?1
           order by ip_to limit 1
        "#,
        ipnumber
    )
    .fetch_one(db())
    .await?;

    Ok(info)
}

/// Lookup the location of an IP address represented in either IPv4 or
/// IPv6 string formats.
pub async fn ip(req: &Request) -> Result<LocationInfo, Error> {
    let textual: String = flexbuffers::from_slice(&req.message.payload)?;

    let addr: IpAddr = textual.parse()?;

    let info = match addr {
        IpAddr::V4(ipv4_addr) => lookup_ipv4(ipv4_addr).await?,
        IpAddr::V6(ipv6_addr) => lookup_ipv6(ipv6_addr).await?,
    };

    Ok(info)
}

/// Lookup the location of an IP address represented in IPv4 string format.
pub async fn ipv4(req: &Request) -> Result<LocationInfo, Error> {
    let textual: String = flexbuffers::from_slice(&req.message.payload)?;

    let addr: Ipv4Addr = textual.parse()?;

    let info = lookup_ipv4(addr).await?;

    Ok(info)
}

/// Lookup the location of an IP address represented in IPv6 string formats.
pub async fn ipv6(req: &Request) -> Result<LocationInfo, Error> {
    let textual: String = flexbuffers::from_slice(&req.message.payload)?;

    let addr: Ipv6Addr = textual.parse()?;

    let info = lookup_ipv6(addr).await?;

    Ok(info)
}

#[cfg(test)]
mod tests {
    use common::message::location::LocationInfo;
    use common::service::errors::DecodeError;

    #[tokio::test]
    #[ignore]
    /// Testing IP address lookup requires that the service be running
    /// and conneected to the NATS backplane on localhost.
    async fn test_ipv4_address_lookup() {
        let nats = common::shared::extra_nats().await;

        let payload =
            flexbuffers::to_vec(String::from("97.107.139.42")).expect("TX Payload encoding failed");

        let msg = nats
            .request("location.ip", payload.into())
            .await
            .expect("Service request resulted in an error")
            .decode_error()
            .expect("Response was an error");

        dbg!(&msg);

        let response: LocationInfo =
            flexbuffers::from_slice(&msg.payload).expect("RX Payload decoding failed");

        assert_eq!(response.region_name, "New Jersey");
    }

    #[tokio::test]
    #[ignore]
    /// Testing IP address lookup requires that the service be running
    /// and conneected to the NATS backplane on localhost.
    async fn test_ipv6_address_lookup() {
        let nats = common::shared::extra_nats().await;

        let payload = flexbuffers::to_vec(String::from("2600:3c03::f03c:95ff:fe8a:d5c2"))
            .expect("TX Payload encoding failed");

        let msg = nats
            .request("location.ip", payload.into())
            .await
            .expect("Service request resulted in an error")
            .decode_error()
            .expect("Response was an error");

        dbg!(&msg);

        let response: LocationInfo =
            flexbuffers::from_slice(&msg.payload).expect("RX Payload decoding failed");

        assert_eq!(response.region_name, "New Jersey");
    }
}
