FROM clux/muslrust:1.76.0 as builder
RUN mkdir /build
ADD Cargo.toml Cargo.lock /build/
ADD src /build/src
WORKDIR /build
RUN --mount=type=cache,target=/build/target \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    cargo build --profile min --bin controller && cp /build/target/x86_64-unknown-linux-musl/min/controller /build/controller


FROM alpine:3.18.2
ARG HELM_VERSION=3.14.2
ARG HELMFILE_VERSION=0.162.0
ARG SOPS_VERSION=3.8.1
ARG HELM_SECRETS_VERSION=4.5.1
ARG HELM_DIFF_VERSION=3.9.5
RUN addgroup -g 1000 controller && adduser -u 1000 -G controller -D controller
RUN wget -O - https://get.helm.sh/helm-v${HELM_VERSION}-linux-amd64.tar.gz | tar -xzO linux-amd64/helm > /usr/local/bin/helm && \
    wget -O - https://github.com/helmfile/helmfile/releases/download/v${HELMFILE_VERSION}/helmfile_${HELMFILE_VERSION}_linux_amd64.tar.gz | tar -xzO helmfile > /usr/local/bin/helmfile && \
    wget https://github.com/getsops/sops/releases/download/v${SOPS_VERSION}/sops-v${SOPS_VERSION}.linux.amd64 -O /usr/local/bin/sops && \
    chmod +x /usr/local/bin/helm /usr/local/bin/helmfile /usr/local/bin/sops
COPY --from=builder --chown=1000:1000 /build/controller /usr/local/bin/
USER 1000:1000
RUN mkdir -p $(helm env HELM_PLUGINS) && wget -O - https://github.com/jkroepke/helm-secrets/releases/download/v${HELM_SECRETS_VERSION}/helm-secrets.tar.gz | tar -C "$(helm env HELM_PLUGINS)" -xzf- && \
    wget -O - https://github.com/databus23/helm-diff/releases/download/v${HELM_DIFF_VERSION}/helm-diff-linux-amd64.tgz | tar -C "$(helm env HELM_PLUGINS)" -xzf-
CMD [ "/usr/local/bin/controller" ]
