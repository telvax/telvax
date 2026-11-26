# Deploying a service

`telvax build --serve` makes one binary with the program and the server, and `--dockerfile` writes the image for it. This
page takes that image to the places services usually run: Docker and Compose, AWS Lambda, Google Cloud Run and Kubernetes.
The commands of each platform are theirs; what Telvax gives them is the same everywhere.

## What the service gives a platform

| A platform asks for | A Telvax service has |
|---|---|
| A port from the environment | It listens on `0.0.0.0` and on `PORT` when that is set (`--port` wins). |
| A health check | `GET /healthz` answers `200`, and `503` once the server has been told to stop. `/service --healthcheck` asks it and exits 0 or 1, for a container with no `curl`. |
| Stopping cleanly | On `SIGTERM` it stops taking connections, waits for the requests and background tasks under way (`TELVAX_SHUTDOWN_WAIT` seconds, 30 by default), then exits. |
| Configuration from the environment | `env("NAME", "default")` in the `serve:` block, `TELVAX_*` variables for the server options, and `std.env` in the program. |
| Logs a platform can read | `TELVAX_LOG_FORMAT=json`: one JSON line per request on the standard error, with the request and trace ids. |
| Metrics and traces | `TELVAX_METRICS=1` serves Prometheus metrics on `/metrics`; `OTEL_EXPORTER_OTLP_ENDPOINT` sends spans over OTLP. |
| A small image | A distroless image, running as the user `nonroot` (65532), not root. |

The image `telvax build --serve app.tvx -o app --dockerfile` writes:

```dockerfile
# syntax=docker/dockerfile:1
ARG TELVAX_IMAGE=ghcr.io/telvax/telvax:latest
FROM ${TELVAX_IMAGE} AS builder
WORKDIR /app
COPY . .
RUN mkdir -p /out && telvax build --serve app.tvx -o /out/service

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /out/service /service
ENV PORT=8080
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=4s --start-period=10s CMD ["/service", "--healthcheck"]
# `nonroot` as a number, so Kubernetes can check `runAsNonRoot` (it refuses a user given by name).
USER 65532:65532
ENTRYPOINT ["/service"]
```

Pin the version of the build image (`--build-arg TELVAX_IMAGE=ghcr.io/telvax/telvax:<version>`) so a rebuild does not
change the compiler. The steps below start from this image.

## Docker and Compose

```sh
docker build -t my-service .
docker run --rm -p 8080:8080 -e TELVAX_LOG_FORMAT=json my-service
```

`docker ps` shows the service as `healthy` once `/healthz` answers, and `docker stop` sends `SIGTERM`: the requests under way
end before it exits. Docker waits 10 seconds before it kills a container, so keep `TELVAX_SHUTDOWN_WAIT` below that, or give
`docker stop --time` more.

With Compose, the health check of the image lets another service wait for this one:

```yaml
services:
  api:
    build: .
    ports: ["8080:8080"]
    environment:
      TELVAX_LOG_FORMAT: json
      TELVAX_SHUTDOWN_WAIT: "8"
      DATABASE_URL: postgres://app:app@db:5432/app
    depends_on:
      db:
        condition: service_healthy
  db:
    image: postgres:17
    environment: { POSTGRES_USER: app, POSTGRES_PASSWORD: app, POSTGRES_DB: app }
    healthcheck:
      test: ["CMD", "pg_isready", "-U", "app"]
      interval: 5s
```

## AWS Lambda

Lambda runs container images, but it calls a function through its own API, not over HTTP. The
[AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter), an extension published by AWS, turns those calls
into HTTP requests to the server in the image, so the same service runs there unchanged. Add the adapter to the image and tell
it where the service listens and how to know it is ready:

```dockerfile
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:0.8.4 /lambda-adapter /opt/extensions/lambda-adapter
COPY --from=builder /out/service /service
ENV PORT=8080 AWS_LWA_PORT=8080 AWS_LWA_READINESS_CHECK_PATH=/healthz
ENTRYPOINT ["/service"]
```

(The `builder` stage is the one above; pin the adapter to a current release.) Push the image to ECR and create the function
from it:

```sh
aws ecr create-repository --repository-name my-service
docker build -t <account>.dkr.ecr.<region>.amazonaws.com/my-service:1 .
docker push <account>.dkr.ecr.<region>.amazonaws.com/my-service:1
aws lambda create-function --function-name my-service --package-type Image \
  --code ImageUri=<account>.dkr.ecr.<region>.amazonaws.com/my-service:1 --role <execution role ARN>
```

A function URL, API Gateway or an Application Load Balancer then sends requests to the endpoints as they are. An event that
is not HTTP (a schedule of EventBridge, a message of SQS) is posted by the adapter to `/events` as JSON, so a job that a
schedule starts is an endpoint:

```telvax
type Run:
    year: Int

@endpoint(POST, "/events")
fn scheduled(run: Run) -> Result[Text, Text]:
    Ok(f"loaded {run.year}")
```

The schedule sends `{"year": 2023}` as its input. Lambda freezes the process between calls, so state kept in memory lasts
only as long as the instance does, and `on_start` runs on each cold start. Logs go to CloudWatch with `TELVAX_LOG_FORMAT=json`.

## Google Cloud Run

Cloud Run builds the image from the directory and sets `PORT` itself:

```sh
gcloud run deploy my-service --source . --region <region> \
  --set-env-vars TELVAX_LOG_FORMAT=json,TELVAX_SHUTDOWN_WAIT=8
```

It sends `SIGTERM` before it stops an instance and kills it 10 seconds later, so keep `TELVAX_SHUTDOWN_WAIT` under 10. Cloud
Run reads the JSON lines of the standard error as structured logs. To check the start, point its startup probe at
`/healthz` (in the service YAML, `startupProbe: httpGet: path: /healthz`). Secrets from Secret Manager arrive as variables
(`--set-secrets DATABASE_URL=db-url:latest`), which `env("DATABASE_URL")` in the `serve:` block, or `std.env` in the
program, reads.

## Kubernetes

The probes and the time to stop are what Kubernetes needs to roll a deployment without dropping requests:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  replicas: 2
  selector:
    matchLabels: { app: my-service }
  template:
    metadata:
      labels: { app: my-service }
    spec:
      terminationGracePeriodSeconds: 40
      securityContext: { runAsNonRoot: true }
      containers:
        - name: api
          image: registry.example.com/my-service:1
          ports: [{ containerPort: 8080 }]
          env:
            - { name: TELVAX_LOG_FORMAT, value: json }
            - { name: TELVAX_METRICS, value: "1" }
            - { name: TELVAX_SHUTDOWN_WAIT, value: "30" }
            - name: DATABASE_URL
              valueFrom: { secretKeyRef: { name: my-service, key: database-url } }
          readinessProbe:
            httpGet: { path: /healthz, port: 8080 }
            periodSeconds: 5
          livenessProbe:
            httpGet: { path: /healthz, port: 8080 }
            periodSeconds: 10
            failureThreshold: 3
---
apiVersion: v1
kind: Service
metadata:
  name: my-service
spec:
  selector: { app: my-service }
  ports: [{ port: 80, targetPort: 8080 }]
```

When a pod is stopped, `/healthz` answers `503`, so the readiness probe takes it out of the service while it finishes what
it was doing. `terminationGracePeriodSeconds` has to be longer than `TELVAX_SHUTDOWN_WAIT`, or the pod is killed before it
is done. With `TELVAX_METRICS=1`, a Prometheus that scrapes pods reads `/metrics` on the same port. The rate limit of
`--rate-limit` counts per process, so with two replicas a client gets up to twice the limit; put the limit in the ingress
when it has to be exact.

## A job, not a service

A program that runs and exits (a batch load, a report) is built with `telvax build --bundle app.tvx -o app --dockerfile`
instead: the image runs `main` with the arguments given to `docker run`, and exits with status 1 when `main` returns an
`Err`. That is what a Kubernetes `CronJob`, a Cloud Run job (`gcloud run jobs deploy`) or an ECS scheduled task runs. On
Lambda, a job is the `/events` endpoint above.
