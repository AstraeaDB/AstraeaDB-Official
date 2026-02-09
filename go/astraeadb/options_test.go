package astraeadb

import (
	"testing"
	"time"
)

func TestDefaultConfig(t *testing.T) {
	cfg := defaultConfig()

	if cfg.host != "127.0.0.1" {
		t.Errorf("host = %q, want 127.0.0.1", cfg.host)
	}
	if cfg.port != 7687 {
		t.Errorf("port = %d, want 7687", cfg.port)
	}
	if cfg.grpcPort != 7688 {
		t.Errorf("grpcPort = %d, want 7688", cfg.grpcPort)
	}
	if cfg.flightPort != 7689 {
		t.Errorf("flightPort = %d, want 7689", cfg.flightPort)
	}
	if cfg.timeout != 10*time.Second {
		t.Errorf("timeout = %v, want 10s", cfg.timeout)
	}
}

func TestWithOptions(t *testing.T) {
	cfg := defaultConfig()

	WithAddress("db.example.com", 9999)(cfg)
	if cfg.host != "db.example.com" || cfg.port != 9999 {
		t.Errorf("WithAddress: host=%q port=%d", cfg.host, cfg.port)
	}

	WithGRPCPort(8888)(cfg)
	if cfg.grpcPort != 8888 {
		t.Errorf("WithGRPCPort: %d, want 8888", cfg.grpcPort)
	}

	WithFlightPort(7777)(cfg)
	if cfg.flightPort != 7777 {
		t.Errorf("WithFlightPort: %d, want 7777", cfg.flightPort)
	}

	WithAuthToken("tok123")(cfg)
	if cfg.authToken != "tok123" {
		t.Errorf("WithAuthToken: %q, want tok123", cfg.authToken)
	}

	WithTimeout(30 * time.Second)(cfg)
	if cfg.timeout != 30*time.Second {
		t.Errorf("WithTimeout: %v, want 30s", cfg.timeout)
	}

	WithDialTimeout(3 * time.Second)(cfg)
	if cfg.dialTimeout != 3*time.Second {
		t.Errorf("WithDialTimeout: %v, want 3s", cfg.dialTimeout)
	}

	WithMaxRetries(5)(cfg)
	if cfg.maxRetries != 5 {
		t.Errorf("WithMaxRetries: %d, want 5", cfg.maxRetries)
	}

	WithReconnect(true)(cfg)
	if !cfg.reconnect {
		t.Error("WithReconnect: want true")
	}
}

func TestAddrHelpers(t *testing.T) {
	cfg := defaultConfig()
	if cfg.addr() != "127.0.0.1:7687" {
		t.Errorf("addr() = %q", cfg.addr())
	}
	if cfg.grpcAddr() != "127.0.0.1:7688" {
		t.Errorf("grpcAddr() = %q", cfg.grpcAddr())
	}
	if cfg.flightAddr() != "127.0.0.1:7689" {
		t.Errorf("flightAddr() = %q", cfg.flightAddr())
	}
}
