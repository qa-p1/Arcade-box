# Local / Public IP Inspector (`arcade.network.ip`)

**Input:** none. **Output:** every local network interface with its addresses, plus the public address when enabled. **Privacy:** NETWORK when the public lookup is on (the default); turn it off for a local-only result.

Local interfaces are listed through the operating system. The public address comes from `api.ipify.org` and becomes the result headline. Gateway and DNS settings are not shown.
