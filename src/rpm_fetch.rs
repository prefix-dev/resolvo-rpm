use std::path::Path;

use reqwest::blocking::Client;
use rpmrepo_metadata::RepositoryReader;
use url::Url;

pub fn fetch_repodata(base_url: Url, target_folder: &Path) {
    let client = Client::new();

    // check if the repomd.xml exists
    if target_folder.join("repodata/repomd.xml").exists() {
        println!("repomd.xml already exists");
        return;
    }

    let url = base_url.join("repodata/repomd.xml").unwrap();
    let mut resp = client.get(url).send().unwrap();

    let path = target_folder.to_path_buf();
    std::fs::create_dir_all(path.join("repodata")).unwrap();
    let mut file = std::fs::File::create(path.join("repodata/repomd.xml")).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let reader = RepositoryReader::new_from_directory(target_folder).unwrap();
    let repomd = reader.repomd();
    // download the other files
    let data = repomd.get_filelist_data();
    let url = base_url
        .join(&data.location_href.to_string_lossy())
        .unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let data = repomd.get_other_data();
    let url = base_url
        .join(&data.location_href.to_string_lossy())
        .unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let data = repomd.get_primary_data();
    let url = base_url
        .join(&data.location_href.to_string_lossy())
        .unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();
}
