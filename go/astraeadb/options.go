package astraeadb

import (
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"os"
	"time"
)

// clientConfig holds all configuration for a client instance.
type clientConfig struct {
	host        string
	port        int
	grpcPort    int
	flightPort  int
	authToken   string
	timeout     time.Duration
	dialTimeout time.Duration
	tlsConfig   *tls.Config
	maxRetries  int
	reconnect   bool
}

func defaultConfig() *clientConfig {
	return &clientConfig{
		host:        "127.0.0.1",
		port:        7687,
		grpcPort:    7688,
		flightPort:  7689,
		timeout:     10 * time.Second,
		dialTimeout: 5 * time.Second,
		maxRetries:  3,
		reconnect:   false,
	}
}

// Option configures a client.
type Option func(*clientConfig)

// WithAddress sets the server host and JSON/TCP port.
func WithAddress(host string, port int) Option {
	return func(c *clientConfig) {
		c.host = host
		c.port = port
	}
}

// WithHost sets the server hostname or IP address.
func WithHost(host string) Option {
	return func(c *clientConfig) {
		c.host = host
	}
}

// WithPort sets the JSON/TCP port (default 7687).
func WithPort(port int) Option {
	return func(c *clientConfig) {
		c.port = port
	}
}

// WithGRPCPort sets the gRPC port (default 7688).
func WithGRPCPort(port int) Option {
	return func(c *clientConfig) {
		c.grpcPort = port
	}
}

// WithFlightPort sets the Arrow Flight port (default 7689).
func WithFlightPort(port int) Option {
	return func(c *clientConfig) {
		c.flightPort = port
	}
}

// WithAuthToken sets the API key for authentication.
func WithAuthToken(token string) Option {
	return func(c *clientConfig) {
		c.authToken = token
	}
}

// WithTimeout sets the per-operation timeout (default 10s).
func WithTimeout(d time.Duration) Option {
	return func(c *clientConfig) {
		c.timeout = d
	}
}

// WithDialTimeout sets the TCP connection timeout (default 5s).
func WithDialTimeout(d time.Duration) Option {
	return func(c *clientConfig) {
		c.dialTimeout = d
	}
}

// WithTLS enables TLS using the given CA certificate file to verify the server.
func WithTLS(caCertFile string) Option {
	return func(c *clientConfig) {
		caCert, err := os.ReadFile(caCertFile)
		if err != nil {
			return
		}
		pool := x509.NewCertPool()
		pool.AppendCertsFromPEM(caCert)
		c.tlsConfig = &tls.Config{
			RootCAs:    pool,
			MinVersion: tls.VersionTLS12,
		}
	}
}

// WithMTLS enables mutual TLS with client certificate authentication.
func WithMTLS(certFile, keyFile, caCertFile string) Option {
	return func(c *clientConfig) {
		cert, err := tls.LoadX509KeyPair(certFile, keyFile)
		if err != nil {
			return
		}
		caCert, err := os.ReadFile(caCertFile)
		if err != nil {
			return
		}
		pool := x509.NewCertPool()
		pool.AppendCertsFromPEM(caCert)
		c.tlsConfig = &tls.Config{
			Certificates: []tls.Certificate{cert},
			RootCAs:      pool,
			MinVersion:   tls.VersionTLS12,
		}
	}
}

// WithTLSConfig sets a custom TLS configuration directly.
func WithTLSConfig(cfg *tls.Config) Option {
	return func(c *clientConfig) {
		c.tlsConfig = cfg
	}
}

// WithMaxRetries sets the maximum retry count for reconnection (default 3).
func WithMaxRetries(n int) Option {
	return func(c *clientConfig) {
		c.maxRetries = n
	}
}

// WithReconnect enables automatic reconnection on connection loss.
func WithReconnect(enabled bool) Option {
	return func(c *clientConfig) {
		c.reconnect = enabled
	}
}

// addr returns the host:port address string.
func (c *clientConfig) addr() string {
	return fmt.Sprintf("%s:%d", c.host, c.port)
}

// grpcAddr returns the host:grpcPort address string.
func (c *clientConfig) grpcAddr() string {
	return fmt.Sprintf("%s:%d", c.host, c.grpcPort)
}

// flightAddr returns the host:flightPort address string.
func (c *clientConfig) flightAddr() string {
	return fmt.Sprintf("%s:%d", c.host, c.flightPort)
}
