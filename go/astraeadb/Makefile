.PHONY: proto test lint build clean

# Generate protobuf Go code from the .proto definition.
proto:
	protoc --go_out=. --go_opt=module=github.com/jimeharrisjr/astraeadb-go \
	       --go-grpc_out=. --go-grpc_opt=module=github.com/jimeharrisjr/astraeadb-go \
	       -I proto proto/astraea.proto

# Run all tests with race detection.
test:
	go test -v -race ./...

# Run golangci-lint (if installed).
lint:
	golangci-lint run ./...

# Build all packages.
build:
	go build ./...

# Remove generated files.
clean:
	rm -f pb/astraea/*.pb.go

# Tidy module dependencies.
tidy:
	go mod tidy
