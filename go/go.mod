module github.com/tabnas/xml/go

go 1.24.7

require github.com/tabnas/jsonic/go v0.0.0

// This package is a grammar plugin built on the @tabnas/jsonic legacy
// shim (the relaxed-JSON grammar engine). Until tabnas/jsonic publishes a
// tagged Go module, depend on a sibling checkout — the same development
// model the TypeScript package uses for `@tabnas/jsonic`
// (file:../../jsonic/ts). Clone https://github.com/tabnas/jsonic as a
// sibling of this repo.
replace github.com/tabnas/jsonic/go => ../../jsonic/go
