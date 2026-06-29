# Build, test and publish the TypeScript (ts/) implementation.
# A Go port is a planned follow-up.

.PHONY: all build test clean reset publish-ts

all: build test

build:
	cd ts && npm run build

test:
	cd ts && npm test

clean:
	rm -rf ts/dist ts/dist-test

reset:
	cd ts && npm run reset

# Publish the TypeScript package at its current package.json version.
publish-ts: test
	cd ts && npm publish --access public
