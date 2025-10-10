use anyhow::Error;
use async_nats::service::Request;
use common::message::email::{
    DatasetInfo, GetDatasetVariablesRequest, GetDatasetVariablesResponse, ListDatasetsResponse,
};

use crate::datasets::DATASET_REGISTRY;

/// Handler for listing all available datasets
#[tracing::instrument(skip_all)]
pub async fn handle_list_datasets(_req: &Request) -> Result<ListDatasetsResponse, Error> {
    let mut datasets = Vec::new();

    for dataset in DATASET_REGISTRY.values() {
        datasets.push(DatasetInfo {
            name: dataset.name().to_string(),
            label: dataset.label().to_string(),
            description: dataset.description().to_string(),
        });
    }

    // Sort by name for consistent ordering
    datasets.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ListDatasetsResponse { datasets })
}

/// Handler for getting variables for a specific dataset
#[tracing::instrument(skip_all)]
pub async fn handle_get_dataset_variables(
    req: &Request,
) -> Result<GetDatasetVariablesResponse, Error> {
    let request: GetDatasetVariablesRequest = flexbuffers::from_slice(&req.message.payload)?;

    let dataset = DATASET_REGISTRY
        .get(&request.dataset_name)
        .ok_or_else(|| anyhow::anyhow!("Dataset '{}' not found", request.dataset_name))?;

    let variables = dataset.variables();

    Ok(GetDatasetVariablesResponse { variables })
}
