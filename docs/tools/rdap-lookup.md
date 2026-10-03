# RDAP / WHOIS Lookup (`arcade.network.rdap`)

**Input:** domain or IP address. **Output:** structured RDAP JSON. **Privacy:** NETWORK; the query is sent to `rdap.org`.

Uses public RDAP services and redacts common contact fields such as email and telephone numbers before returning data. WHOIS fallback, registry selection controls, and more complete privacy-field normalization remain future work.
