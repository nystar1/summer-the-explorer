# Summer the Explorer

# Components
This project has two main tools: Oculus, which syncs data from the Hack Club official APIs (SoM APIs, Hackatime etc.) to a Postgres database (enabled with pgvector for embeddings) and Explorer, which is the (kind of) thin client API that serves the data. Oculus is independent of the API and can be used for other purposes.

# Docs
Documentation and playground available [here](https://exploresummer.livingfor.me/v1/docs#tag/comments)!

# Build
`cargo build --workspace`

# Platforms
Tested on Linux and macOS. Should work on Windows with minor changes.