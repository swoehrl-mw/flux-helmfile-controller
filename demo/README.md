# flux-helmfile-controller demo

## Edit secrets

```bash
export SOPS_AGE_RECIPIENTS=age1qgfdsn58m70ja4pr20hjg49tc6zp76mxef8xfrrep2lh5ynr83fsp3nuvs
export SOPS_AGE_KEY_FILE=$(pwd)/age.agekey
helm secrets edit secrets1.enc.yaml
helm secrets edit secrets2.enc.yaml
```

## Deploy via helmfile manually

```bash
export SOPS_AGE_KEY_FILE=$(pwd)/age.agekey
helmfile apply
```
