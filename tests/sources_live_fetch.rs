use rscholar::server::config::ServerConfig;
use rscholar::{arxiv, pubmed, xrxiv};

fn load_config() -> ServerConfig {
    ServerConfig::load_from_file("config.toml").unwrap_or_default()
}

fn print_header(name: &str) {
    println!("\n================ {} ================", name);
}

#[tokio::test]
#[ignore = "requires external network access"]
async fn test_arxiv_live_fetch_small_batch() {
    let config = load_config();
    let options = arxiv::ArxivQueryOptions {
        max_results: config.search.arxiv.max_results.min(5),
        page_size: config.search.arxiv.page_size.min(5),
        sort_by: config.search.arxiv.sort_by.clone(),
        sort_order: config.search.arxiv.sort_order.clone(),
        timeout_secs: config.search.arxiv.timeout_sec,
        request_delay_ms: 500,
    };

    let papers = arxiv::search_papers("ai", &options)
        .await
        .expect("arxiv live request should not fail");
    print_header("arXiv");
    println!("fetched={} (max={})", papers.len(), options.max_results);
    for (idx, paper) in papers.iter().take(3).enumerate() {
        println!(
            "#{} title={} | year={} | doi={} | url={}",
            idx + 1,
            paper.title,
            paper.year,
            paper.doi,
            paper.url
        );
    }
    assert!(papers.len() <= 5);
}

#[tokio::test]
#[ignore = "requires external network access"]
async fn test_pubmed_live_fetch_small_batch() {
    let config = load_config();
    let options = pubmed::PubMedQueryOptions {
        max_results: config.search.pubmed.max_results.min(5),
        page_size: config.search.pubmed.page_size.min(5),
        timeout_secs: config.search.pubmed.timeout_sec,
        api_key: if config.search.pubmed.api_key.trim().is_empty() {
            None
        } else {
            Some(config.search.pubmed.api_key.clone())
        },
        tool: Some(config.search.pubmed.tool.clone()),
        email: if config.search.pubmed.email.trim().is_empty() {
            None
        } else {
            Some(config.search.pubmed.email.clone())
        },
        delay_no_key_ms: config.search.pubmed.delay_no_key_ms,
        delay_with_key_ms: config.search.pubmed.delay_with_key_ms,
    };

    let papers = pubmed::search_papers("ai", &options)
        .await
        .expect("pubmed live request should not fail");
    print_header("PubMed");
    println!("fetched={} (max={})", papers.len(), options.max_results);
    for (idx, paper) in papers.iter().take(3).enumerate() {
        println!(
            "#{} title={} | year={} | doi={} | url={}",
            idx + 1,
            paper.title,
            paper.year,
            paper.doi,
            paper.url
        );
    }
    assert!(papers.len() <= 5);
}

#[tokio::test]
#[ignore = "requires external network access"]
async fn test_biorxiv_live_fetch_small_batch() {
    let config = load_config();
    let options = xrxiv::XRxivQueryOptions {
        max_results: config.search.xrxiv.biorxiv_max_results.min(5),
        start_date: config.search.xrxiv.start_date.clone(),
        end_date: if config.search.xrxiv.end_date.trim().is_empty() {
            chrono::Utc::now().format("%Y-%m-%d").to_string()
        } else {
            config.search.xrxiv.end_date.clone()
        },
        timeout_secs: config.search.xrxiv.timeout_sec,
        request_delay_ms: config.search.xrxiv.request_delay_ms,
        max_retries: config.search.xrxiv.max_retries,
    };

    let papers = xrxiv::search_papers(xrxiv::XRxivServer::BioRxiv, "ai", &options)
        .await
        .expect("biorxiv live request should not fail");
    print_header("bioRxiv");
    println!("fetched={} (max={})", papers.len(), options.max_results);
    for (idx, paper) in papers.iter().take(3).enumerate() {
        println!(
            "#{} title={} | year={} | doi={} | url={}",
            idx + 1,
            paper.title,
            paper.year,
            paper.doi,
            paper.url
        );
    }
    assert!(papers.len() <= 5);
}

#[tokio::test]
#[ignore = "requires external network access"]
async fn test_medrxiv_live_fetch_small_batch() {
    let config = load_config();
    let options = xrxiv::XRxivQueryOptions {
        max_results: config.search.xrxiv.medrxiv_max_results.min(5),
        start_date: config.search.xrxiv.start_date.clone(),
        end_date: if config.search.xrxiv.end_date.trim().is_empty() {
            chrono::Utc::now().format("%Y-%m-%d").to_string()
        } else {
            config.search.xrxiv.end_date.clone()
        },
        timeout_secs: config.search.xrxiv.timeout_sec,
        request_delay_ms: config.search.xrxiv.request_delay_ms,
        max_retries: config.search.xrxiv.max_retries,
    };

    let papers = xrxiv::search_papers(xrxiv::XRxivServer::MedRxiv, "ai", &options)
        .await
        .expect("medrxiv live request should not fail");
    print_header("medRxiv");
    println!("fetched={} (max={})", papers.len(), options.max_results);
    for (idx, paper) in papers.iter().take(3).enumerate() {
        println!(
            "#{} title={} | year={} | doi={} | url={}",
            idx + 1,
            paper.title,
            paper.year,
            paper.doi,
            paper.url
        );
    }
    assert!(papers.len() <= 5);
}

#[tokio::test]
#[ignore = "requires external network access"]
async fn test_sources_live_verbose_report() {
    let config = load_config();
    let target = 100usize;

    // arXiv
    let arxiv_opts = arxiv::ArxivQueryOptions {
        max_results: config.search.arxiv.max_results.min(target),
        page_size: config.search.arxiv.page_size.min(100),
        sort_by: config.search.arxiv.sort_by.clone(),
        sort_order: config.search.arxiv.sort_order.clone(),
        timeout_secs: config.search.arxiv.timeout_sec,
        request_delay_ms: 300,
    };
    let arxiv_papers = arxiv::search_papers("ai", &arxiv_opts)
        .await
        .expect("arxiv live request should not fail");
    print_header("REPORT arXiv");
    println!("count={} (target={})", arxiv_papers.len(), arxiv_opts.max_results);
    for (i, p) in arxiv_papers.iter().take(10).enumerate() {
        println!("{} | {} | {} | {}", i + 1, p.year, p.title, p.url);
    }

    // PubMed
    let pubmed_opts = pubmed::PubMedQueryOptions {
        max_results: config.search.pubmed.max_results.min(target),
        page_size: config.search.pubmed.page_size.min(100),
        timeout_secs: config.search.pubmed.timeout_sec,
        api_key: if config.search.pubmed.api_key.trim().is_empty() {
            None
        } else {
            Some(config.search.pubmed.api_key.clone())
        },
        tool: Some(config.search.pubmed.tool.clone()),
        email: if config.search.pubmed.email.trim().is_empty() {
            None
        } else {
            Some(config.search.pubmed.email.clone())
        },
        delay_no_key_ms: config.search.pubmed.delay_no_key_ms,
        delay_with_key_ms: config.search.pubmed.delay_with_key_ms,
    };
    let pubmed_papers = pubmed::search_papers("ai", &pubmed_opts)
        .await
        .expect("pubmed live request should not fail");
    print_header("REPORT PubMed");
    println!("count={} (target={})", pubmed_papers.len(), pubmed_opts.max_results);
    for (i, p) in pubmed_papers.iter().take(10).enumerate() {
        println!("{} | {} | {} | {}", i + 1, p.year, p.title, p.url);
    }

    // bioRxiv
    let xrxiv_common = xrxiv::XRxivQueryOptions {
        max_results: config
            .search
            .xrxiv
            .biorxiv_max_results
            .min(config.search.xrxiv.medrxiv_max_results)
            .min(target),
        start_date: config.search.xrxiv.start_date.clone(),
        end_date: if config.search.xrxiv.end_date.trim().is_empty() {
            chrono::Utc::now().format("%Y-%m-%d").to_string()
        } else {
            config.search.xrxiv.end_date.clone()
        },
        timeout_secs: config.search.xrxiv.timeout_sec,
        request_delay_ms: config.search.xrxiv.request_delay_ms,
        max_retries: config.search.xrxiv.max_retries,
    };
    let biorxiv_papers =
        xrxiv::search_papers(xrxiv::XRxivServer::BioRxiv, "ai", &xrxiv_common)
            .await
            .expect("biorxiv live request should not fail");
    print_header("REPORT bioRxiv");
    println!("count={} (target={})", biorxiv_papers.len(), xrxiv_common.max_results);
    for (i, p) in biorxiv_papers.iter().take(10).enumerate() {
        println!("{} | {} | {} | {}", i + 1, p.year, p.title, p.url);
    }

    // medRxiv
    let medrxiv_papers =
        xrxiv::search_papers(xrxiv::XRxivServer::MedRxiv, "ai", &xrxiv_common)
            .await
            .expect("medrxiv live request should not fail");
    print_header("REPORT medRxiv");
    println!("count={} (target={})", medrxiv_papers.len(), xrxiv_common.max_results);
    for (i, p) in medrxiv_papers.iter().take(10).enumerate() {
        println!("{} | {} | {} | {}", i + 1, p.year, p.title, p.url);
    }
}
