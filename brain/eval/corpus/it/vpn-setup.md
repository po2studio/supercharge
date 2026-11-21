# VPN setup

We use WireGuard. Install the client from the self-service portal, then run:

```bash
# generate a key pair on your laptop
wg genkey | tee privatekey | wg pubkey > publickey
# send publickey to it@hrafn.example, never the private one
# after IT replies, import the config
sudo wg-quick up hrafn
# check that the tunnel is alive
wg show
```

The VPN is required on any network that is not the office wifi. Split tunnelling is enabled, so streaming music is fine.
