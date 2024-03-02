# Flux Helmfile Controller

The flux-helmfile-controller is intended as an addon to [FluxCD](https://fluxcd.io/). Alongside the existing `Kustomization` and `HelmRelease` custom resources it introduces a new `Helmfile` resource that allows doing deployments via [helmfile](https://github.com/helmfile/helmfile). This introduces the powers of values templating and encryption to Flux Helm deployments.

This project is currently still in development and a work-in-progress. Use with caution.

## Quickstart

Prerequisites:

* A kubernetes cluster with at least version 1.25.0 (for testing you can quickly set one up using `k3d cluster create`).
* `kubectl` installed on your machine and its kubeconfig pointing to the cluster.
* `helm` installed in your machine, to verify deployment.

To quickly install and test the controller, follow these steps:

1. Clone this repository: `git clone https://github.com/swoehrl-mw/flux-helmfile-controller.git && cd flux-helmfile-controller`.
2. Install flux: `kubectl apply -f https://github.com/fluxcd/flux2/releases/latest/download/install.yaml`.
3. Install the helmfile-controller: `kubectl apply -f https://github.com/swoehrl-mw/flux-helmfile-controller/releases/latest/download/manifests.yaml`.
4. Deploy the example age key secret: `kubectl create secret generic sops-age-key --namespace=default --from-file=age.agekey=demo/age.agekey`.
5. Deploy the example `GitRepository` and `Helmfile`: `kubectl apply -f demo/repo.yaml`.
6. Wait a few seconds and then check the status of the helmfile deployment: `kubectl describe helmfile demo`. It should show the following:

    ```plain
    Status:
    Last Update:  2024-01-03T10:49:45Z
    Reason:       <nil>
    Status:       successful
    ```

7. Verify helmfile installed its chart: `helm status demo`:

    ```plain
    NAME: demo
    LAST DEPLOYED: Wed Jan  3 11:49:45 2024
    NAMESPACE: default
    STATUS: deployed
    REVISION: 1
    TEST SUITE: None
    ```

## Installing the controller

The helmfile-controller requires a running installation of FluxCD >= 2.0 (providing the `GitRepository` custom resource). The controller should be installed into the same namespace as flux (by default `flux-system`) as otherwise there might be communication problems with the flux-source-controller.

All needed manifests are in this repository in the `manifests` folder and are using the `flux-system` namespace. You can get the latest released version using `https://github.com/swoehrl-mw/flux-helmfile-controller/releases/latest/download/manifests.yaml`.

## Using the controller

To use the helmfile-controller you will need a git repository with a `helmfile.yaml`. Create a Flux `GitRepository` object pointing to that repo. Then create a `Helmfile` object pointing to that repo object.

```yaml
apiVersion: flux.maibornwolff.de/v1alpha1
kind: Helmfile
metadata:
  name: my-helmfile
  namespace: default # must be in the same namespace as the GitRepository
spec:
  interval: 10m0s # Optional, How often should the controller run `helmfile apply` even if there are no changes
  serviceAccountName: # Optional, name of a seviceaccount to impersonate for helmfile operations
  sourceRef:
    kind: GitRepository
    name: mytest # Name of the GitRepository object, must be in same namespace as Helmfile object
  path: my/path # Optional, path to the directory where helmfile.yaml is located, from repo root, can be skipped if helmfile.yaml is in root
  environment: default # Optional, environment to use for `helmfile -e`
  decryption: # Optional, only needed if the helmfile uses encrypted values/secrets
    provider: sops-age # Provider to use for decryption, currently only sops-age is supported
    secretRef:
      name: sops-age-key # Name of a secret in the same namespace that has a key `age.agekey` with the private age key
  options: # Optional, options related to helmfile execution
    timeout: 10m # Optional, timeout after which apply/sync/destroy operations are aborted
    retries: -1 # Optional, number of retries in case of helmfile failures, 0 means never, negative means retry forever, default is retry forever
    prune: false # Optional, if set to true `helmfile destroy` will be run on deletion of object, otherwise the helm releases will be kept to avoid accidental deletion
```

To create the secret for decryption use the following command: `kubectl create secret generic sops-age-key --namespace=default --from-file=age.agekey=age.agekey`.

After the object has been created, the controller will run `helmfile sync`. And for each change to either the `Helmfile` object or for any new revision of the git repo or after `spec.interval` the controller will reconcile by running `helmfile apply`.

If you want to force an immediate reconcile or want to run a `sync` instead of `apply`, add an action label to the object: `kubectl label helmfile my-helmfile controller/action=sync`. The controller will then execute an immediate `helmfile sync` and will delete the label afterwards.

To uninstall the releases, simply delete the `Helmfile` object after having updated with `options.prune: true`. The controller will then run `helmfile destroy`. Note that the decryption secret and the `GitRepository` must still exist for the controller to successfully run. If the secret is missing or the GitRepository object has been deleted as well the controller will silently end the reconcile instead of blocking. You must then delete the helm releases manually.

## Developing the controller

To develop and run the controller locally you need the following prerequisites:

* Rust and `cargo` installed (use the current stable version)
* A local kubernetes cluster (e.g. using k3d: `k3d cluster create dev`)
* `kubectl`, `helm` and `helmfile` installed

To run the service locally follow these steps:

0. Make sure your kubernetes cluster is stared and your kubeconfig points at it
1. Install flux: `kubectl apply -f https://github.com/fluxcd/flux2/releases/latest/download/install.yaml`
2. Build the controller: `cargo build`
3. Generate the CRD yaml and apply it: `cargo run --bin devhelper && kubectl apply -f manifests/crd.yaml`
4. In a separate terminal start a port-forward for the flux source controller: `kubectl port-forward svc/source-controller -n flux-system 8080:80`
5. Start the controller: `SOURCE_CONTROLLER_HOST=localhost:8080 cargo run --bin controller`
6. From a separate terminal you can now apply `GitRepository` and `Helmfile` manifests
