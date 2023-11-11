use std::path::{PathBuf, Path};
use rpmrepo_metadata::RepositoryReader;
use url::Url;
use reqwest::blocking::Client;

fn fetch_repodata(base_url: Url, target_folder: &Path) {
    let client = Client::new();

    // check if the repomd.xml exists
    if target_folder.join("repodata/repomd.xml").exists() {
        println!("repomd.xml already exists");
        return;
    }

    let url = base_url.clone().join("repodata/repomd.xml").unwrap();
    let mut resp = client.get(url).send().unwrap();

    let path = target_folder.to_path_buf();
    std::fs::create_dir_all(path.join("repodata")).unwrap();
    let mut file = std::fs::File::create(path.join("repodata/repomd.xml")).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let reader = RepositoryReader::new_from_directory(&target_folder).unwrap();
    let repomd = reader.repomd();
    // download the other files
    let data = repomd.get_filelist_data();
    let url = base_url.clone().join(&data.location_href.to_string_lossy()).unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();    
    std::io::copy(&mut resp, &mut file).unwrap();

    let data = repomd.get_other_data();
    let url = base_url.clone().join(&data.location_href.to_string_lossy()).unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let data = repomd.get_primary_data();
    let url = base_url.clone().join(&data.location_href.to_string_lossy()).unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();
}

fn print_pkgs(path: &Path) {
    let reader = RepositoryReader::new_from_directory(&path).unwrap();

    for pkg in reader.iter_packages().unwrap() {
        let pkg = pkg.unwrap();
        println!("{}-{}-{}-{}",
                 pkg.name(),
                 pkg.version(),
                 pkg.release(),
                 pkg.arch());

        println!("Provides:   {:?}", pkg.provides());
        println!("Recommends: {:?}", pkg.recommends());
        println!("Suggests:   {:?}", pkg.suggests());
        println!("Conflicts:  {:?}", pkg.conflicts());
    }
}



fn main() {
    let target_folder = PathBuf::from("./fedora");
    let url = Url::parse("https://mirrors.xtom.de/fedora/updates/38/Everything/x86_64/").unwrap();
    fetch_repodata(url, &target_folder);
    print_pkgs(&target_folder);
}
