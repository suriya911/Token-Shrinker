# Session policy

A session is active only while `expiresAtMs > nowMs`. Equality is expired because authorization is evaluated at the expiration boundary.
