mod crd;

use kube::CustomResourceExt;

fn main() {
    println!("Generating CRD");
    let data = serde_yaml::to_string(&crd::Helmfile::crd())
        .expect("Could not generate yaml from CRD definition");
    std::fs::write("manifests/crd.yaml", data).expect("Failed to write crd yaml to manifests");
}
