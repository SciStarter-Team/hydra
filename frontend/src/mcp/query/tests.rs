use super::*;

fn response(columns: &[(&str, &str)], rows: &[&str], truncated: Truncation) -> QueryResponse {
    QueryResponse {
        columns: columns
            .iter()
            .map(|(name, declared_type)| ColumnInfo {
                name: name.to_string(),
                declared_type: declared_type.to_string(),
            })
            .collect(),
        rows: rows.iter().map(|row| row.to_string()).collect(),
        truncated,
    }
}

#[test]
fn every_target_is_reachable_by_name() {
    for target in TARGETS {
        assert_eq!(
            Target::find(target.name).expect("a known name").name,
            target.name
        );
    }
}

/// The frontend database is the one with a copy per instance, so it is
/// the one that must not be queried through a queue group that would
/// pick a copy at random. Every other database has a single owner.
#[test]
fn only_the_frontend_database_is_queried_in_process() {
    let local: Vec<&str> = TARGETS
        .iter()
        .filter(|target| matches!(target.source, Source::Local))
        .map(|target| target.name)
        .collect();

    assert_eq!(local, ["frontend"]);
}

/// A result from the frontend database has to say which copy answered;
/// for the single-owner databases there is nothing to say.
#[test]
fn only_a_replicated_database_reports_which_copy_answered() {
    let frontend = Target::find("frontend").expect("frontend");
    let forum = Target::find("forum").expect("forum");

    assert_eq!(
        frontend.answered_by(),
        Some(crate::peering::SERVER_ID_STR.as_str())
    );
    assert_eq!(forum.answered_by(), None);

    let rendered = render(
        frontend,
        &response(&[("n", "INTEGER")], &[r#"{"n":1}"#], Truncation::None),
    )
    .expect("rendered");

    let value: Value = serde_json::from_str(&rendered).expect("valid JSON");

    assert_eq!(value["answered_by"], *crate::peering::SERVER_ID_STR);

    let rendered = render(
        forum,
        &response(&[("n", "INTEGER")], &[r#"{"n":1}"#], Truncation::None),
    )
    .expect("rendered");

    let value: Value = serde_json::from_str(&rendered).expect("valid JSON");

    assert!(value.get("answered_by").is_none(), "{rendered}");
}

/// Naming a database that does not exist should say which ones do,
/// rather than leaving the caller to guess.
#[test]
fn an_unknown_database_is_named_along_with_the_known_ones() {
    let error = Target::find("orrery")
        .expect_err("an unknown name")
        .to_string();

    assert!(error.contains("orrery"), "{error}");

    for target in TARGETS {
        assert!(error.contains(target.name), "{error}");
    }
}

/// The tool's enum is what an agent picks a database from, so it has to
/// list exactly the databases the tool can reach.
#[test]
fn the_tool_schema_offers_exactly_the_known_databases() {
    let tools = tool_definitions();

    let query = tools
        .iter()
        .find(|tool| tool["name"] == "run_select_query")
        .expect("run_select_query is defined");

    let offered = query["inputSchema"]["properties"]["database"]["enum"]
        .as_array()
        .expect("an enum of database names");

    let known: Vec<Value> = TARGETS.iter().map(|target| json!(target.name)).collect();

    assert_eq!(offered, &known);
}

#[tokio::test]
async fn an_unknown_tool_is_not_claimed() {
    assert!(dispatch("get_content", json!({})).await.is_none());
}

/// Rows keep the key order the query selected, which is the whole reason
/// they travel as text rather than as parsed values.
#[test]
fn rendered_rows_keep_their_column_order() {
    let target = Target::find("frontend").expect("frontend");

    let rendered = render(
        target,
        &response(
            &[("zebra", "INTEGER"), ("aardvark", "TEXT")],
            &[r#"{"zebra":2,"aardvark":"text"}"#],
            Truncation::None,
        ),
    )
    .expect("rendered");

    assert!(
        rendered.find("\"zebra\"") < rendered.find("\"aardvark\""),
        "row keys are out of order: {rendered}"
    );

    let value: Value = serde_json::from_str(&rendered).expect("valid JSON");

    assert_eq!(value["database"], "frontend");
    assert_eq!(value["row_count"], 1);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["columns"][0]["name"], "zebra");
    assert_eq!(value["columns"][0]["type"], "INTEGER");
    assert_eq!(value["rows"][0]["aardvark"], "text");
}

/// A truncated result has to say so in a way that tells the caller what
/// to do differently.
#[test]
fn truncation_is_reported_with_its_remedy() {
    assert_eq!(truncation(Truncation::None), json!(false));

    let row_limit = truncation(Truncation::RowLimit);
    let row_limit = row_limit.as_str().expect("an explanation");

    assert!(row_limit.contains("max_rows"), "{row_limit}");

    let byte_budget = truncation(Truncation::ByteBudget);
    let byte_budget = byte_budget.as_str().expect("an explanation");

    assert!(byte_budget.contains("fewer columns"), "{byte_budget}");
}

/// The query tools are staff-only: they must not appear on the public
/// endpoint, which has no authentication at all.
#[test]
fn the_tools_are_not_offered_publicly() {
    let public = crate::mcp::get_tools().to_string();

    for tool in tool_definitions() {
        let name = tool["name"].as_str().expect("a tool name");

        assert!(
            !public.contains(name),
            "{name} is reachable without authentication"
        );
    }
}

/// ...and they are offered on the authenticated one.
#[test]
fn the_tools_are_offered_to_authenticated_callers() {
    let authenticated = crate::mcp::content::get_tools().to_string();

    for tool in tool_definitions() {
        let name = tool["name"].as_str().expect("a tool name");

        assert!(authenticated.contains(name), "{name} is not offered");
    }
}

/// A spatially enabled database holds more of SpatiaLite's bookkeeping
/// than of its own tables, and listing all of it buries the answer to
/// the question `list_databases` was asked.
#[test]
fn spatialite_bookkeeping_is_left_out_of_a_listing() {
    // Every one of these was in the live listing for the frontend and
    // user databases, and none of them is created by a migration.
    for table in [
        "ElementaryGeometries",
        "ISO_metadata_view",
        "KNN2",
        "SE_vector_styles",
        "SpatialIndex",
        "data_licenses",
        "geom_cols_ref_sys",
        "geometry_columns_field_infos",
        "networks",
        "raster_coverages_srid",
        "rl2map_configurations",
        "spatial_ref_sys_aux",
        "spatialite_history",
        "sql_statements_log",
        "stored_variables",
        "topologies",
        "vector_layers_statistics",
        "views_geometry_columns_auth",
        "virts_geometry_columns",
        "wms_getcapabilities",
    ] {
        assert!(Dialect::Sqlite.is_metadata(table), "{table} was listed");
    }
}

/// ...and the filter must not swallow the tables it sits next to.
#[test]
fn application_tables_survive_the_listing_filter() {
    for table in [
        "_sqlx_migrations",
        "entities",
        "events",
        "geocode_cache",
        "misc",
        "project_topics",
        "projects",
        "site_settings",
        "users",
        "webinars",
    ] {
        assert!(!Dialect::Sqlite.is_metadata(table), "{table} was hidden");
    }

    // The analytics context has no SpatiaLite in it, and its table names
    // are the caller's whole map of what is there.
    assert!(!Dialect::DataFusion.is_metadata("spatial_ref_sys"));
}

/// The caller must not give up before the endpoint it is waiting on
/// does, or a query that ran its full time would have its answer thrown
/// away by the one who asked for it.
#[test]
fn the_caller_waits_longer_than_the_endpoint_does() {
    assert!(REQUEST_TIMEOUT > common::query::QUERY_TIMEOUT);
}
