package protocol

import (
	"io"
	"net"
	"testing"
)

func TestSendReceive(t *testing.T) {
	client, server := net.Pipe()
	defer client.Close()
	defer server.Close()

	cc := NewConn(client)
	sc := NewConn(server)

	// Send from client, receive on server.
	go func() {
		if err := cc.Send(map[string]string{"hello": "world"}); err != nil {
			t.Errorf("Send: %v", err)
		}
	}()

	var msg map[string]string
	if err := sc.Receive(&msg); err != nil {
		t.Fatalf("Receive: %v", err)
	}
	if msg["hello"] != "world" {
		t.Errorf("msg = %v, want {hello: world}", msg)
	}
}

func TestReceiveEOF(t *testing.T) {
	client, server := net.Pipe()
	sc := NewConn(server)

	// Close the client side immediately.
	client.Close()

	var msg map[string]any
	err := sc.Receive(&msg)
	if err != io.EOF {
		t.Errorf("err = %v, want io.EOF", err)
	}
}

func TestRaw(t *testing.T) {
	client, _ := net.Pipe()
	defer client.Close()

	cc := NewConn(client)
	if cc.Raw() != client {
		t.Error("Raw() should return the underlying conn")
	}
}

func TestRoundTrip(t *testing.T) {
	client, server := net.Pipe()
	defer client.Close()
	defer server.Close()

	cc := NewConn(client)
	sc := NewConn(server)

	type payload struct {
		ID    int    `json:"id"`
		Name  string `json:"name"`
		Score float64 `json:"score"`
	}

	go func() {
		cc.Send(payload{ID: 42, Name: "test", Score: 3.14})
	}()

	var p payload
	if err := sc.Receive(&p); err != nil {
		t.Fatalf("Receive: %v", err)
	}
	if p.ID != 42 || p.Name != "test" || p.Score != 3.14 {
		t.Errorf("got %+v", p)
	}
}
