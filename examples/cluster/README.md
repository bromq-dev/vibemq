# VibeMQ Cluster Example

This example demonstrates horizontal clustering with VibeMQ nodes behind a load balancer.

## Architecture

```
                    ┌─────────────┐
   Clients ────────►│   HAProxy   │
                    │   (L4 LB)   │
                    └──────┬──────┘
                           │ PROXY protocol v2
              ┌────────────┼────────────┐
              ▼            ▼            ▼
         ┌────────┐   ┌────────┐   ┌────────┐
         │VibeMQ 1│◄─►│VibeMQ 2│◄─►│VibeMQ 3│
         └────────┘   └────────┘   └────────┘
              │            │            │
              └────────────┴────────────┘
                   Gossip + Peer TCP
```

- **HAProxy** load balances MQTT clients with PROXY protocol for real client IPs
- **Gossip (UDP 7946)** for node discovery via chitchat protocol
- **Peer TCP (7947)** for message forwarding between nodes
- **Metrics (9090)** for health checks and Prometheus scraping

## Kubernetes (kind)

For local testing with [kind](https://kind.sigs.k8s.io/):

```bash
# Create cluster with port mappings
kind create cluster --name vibemq --config kind-config.yaml

# Build and load VibeMQ image
docker build -t vibemq:latest ../..
kind load docker-image vibemq:latest --name vibemq

# Deploy
kubectl apply -f kubernetes.yaml
kubectl scale deployment vibemq --replicas=3

# Wait for pods
kubectl get pods -l app=vibemq -w

# Test - connects directly, no port-forward needed!
mosquitto_pub -h localhost -p 1883 -t test -m "hello"
curl localhost:9090/health

# Check cluster formed
kubectl logs -l app=vibemq | grep "cluster peer"

# Clean up
kind delete cluster --name vibemq
```

## Kubernetes (Cloud)

For cloud deployments with a LoadBalancer that supports PROXY protocol:

1. Edit `kubernetes.yaml`:
   - Change Service type to `LoadBalancer`
   - Add cloud-specific annotations for PROXY protocol
   - Uncomment `[server.proxy_protocol]` in the ConfigMap

Example for AWS NLB:
```yaml
metadata:
  annotations:
    service.beta.kubernetes.io/aws-load-balancer-type: "nlb"
    service.beta.kubernetes.io/aws-load-balancer-proxy-protocol: "*"
spec:
  type: LoadBalancer
```

## Docker Compose

```bash
# Start cluster with HAProxy
docker compose up --build

# Test through load balancer
mosquitto_pub -h localhost -p 1883 -t test -m "hello"

# Check HAProxy stats
open http://localhost:8404/stats

# Check cluster formed
docker compose logs vibemq | grep "cluster peer"
```

## How It Works

1. **Node Discovery**: Nodes use chitchat gossip protocol to discover each other
2. **Subscription Sync**: Each node advertises its subscriptions via gossip state
3. **Message Routing**: Messages are forwarded to nodes with matching subscriptions
4. **Loop Prevention**: Messages include origin node ID to prevent infinite loops
5. **Client IP Preservation**: HAProxy sends real client IP via PROXY protocol

## Configuration

```toml
# Enable PROXY protocol for load balancer
[server.proxy_protocol]
enabled = true
timeout = 5

# Cluster configuration
[[cluster]]
enabled = true
gossip_addr = "0.0.0.0:7946"
peer_addr = "0.0.0.0:7947"
seeds = ["vibemq-headless:7946"]  # Headless service for discovery

# Health endpoint for load balancer
[metrics]
enabled = true
bind = "0.0.0.0:9090"
```

## Testing

### Cross-Node Pub/Sub
```bash
# Subscribe (connects to any node via LB)
mosquitto_sub -h localhost -p 1883 -t "test/#" -v &

# Publish (may hit different node)
mosquitto_pub -h localhost -p 1883 -t "test/hello" -m "Hello!"

# Message appears regardless of which nodes handle sub/pub
```

### Health Check
```bash
curl localhost:9090/health   # Returns "OK"
curl localhost:9090/metrics  # Prometheus metrics
```

### Scaling
```bash
# Kubernetes
kubectl scale deployment vibemq --replicas=5

# Docker Compose
docker compose up -d --scale vibemq=5
```

## Files

| File | Description |
|------|-------------|
| `compose.yml` | Docker Compose with HAProxy + 3 VibeMQ nodes |
| `haproxy.cfg` | HAProxy config with PROXY protocol and health checks |
| `config.toml` | VibeMQ cluster config (shared by all nodes) |
| `kubernetes.yaml` | K8s Deployment, Services, HPA, ConfigMap |
| `kind-config.yaml` | Kind cluster config with port mappings |
