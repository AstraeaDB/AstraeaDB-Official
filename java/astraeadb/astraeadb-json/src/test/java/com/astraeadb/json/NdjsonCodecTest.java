package com.astraeadb.json;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import org.junit.jupiter.api.Test;

import java.io.*;
import java.net.ServerSocket;
import java.net.Socket;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class NdjsonCodecTest {

    private final ObjectMapper mapper = new ObjectMapper();

    @Test
    void sendAndReceive() throws Exception {
        try (ServerSocket ss = new ServerSocket(0)) {
            Thread serverThread = new Thread(() -> {
                try (Socket server = ss.accept()) {
                    BufferedReader reader = new BufferedReader(
                        new InputStreamReader(server.getInputStream()));
                    String line = reader.readLine();
                    // Echo it back
                    OutputStream out = server.getOutputStream();
                    out.write(line.getBytes());
                    out.write('\n');
                    out.flush();
                } catch (IOException e) {
                    throw new UncheckedIOException(e);
                }
            });
            serverThread.setDaemon(true);
            serverThread.start();

            try (Socket client = new Socket("127.0.0.1", ss.getLocalPort())) {
                NdjsonCodec codec = new NdjsonCodec(client, mapper);

                ObjectNode request = mapper.createObjectNode();
                request.put("type", "Ping");
                codec.send(request);

                JsonNode response = codec.receive();
                assertThat(response.path("type").asText()).isEqualTo("Ping");
            }
        }
    }

    @Test
    void receiveEof() throws Exception {
        try (ServerSocket ss = new ServerSocket(0)) {
            Thread serverThread = new Thread(() -> {
                try (Socket server = ss.accept()) {
                    // Close immediately without sending anything
                    server.close();
                } catch (IOException e) {
                    throw new UncheckedIOException(e);
                }
            });
            serverThread.setDaemon(true);
            serverThread.start();

            try (Socket client = new Socket("127.0.0.1", ss.getLocalPort())) {
                NdjsonCodec codec = new NdjsonCodec(client, mapper);
                assertThatThrownBy(codec::receive)
                    .isInstanceOf(IOException.class)
                    .hasMessageContaining("Connection closed by server");
            }
        }
    }

    @Test
    void roundTrip() throws Exception {
        try (ServerSocket ss = new ServerSocket(0)) {
            Thread serverThread = new Thread(() -> {
                try (Socket server = ss.accept()) {
                    BufferedReader reader = new BufferedReader(
                        new InputStreamReader(server.getInputStream()));
                    String line = reader.readLine();
                    // Parse, modify, and send back
                    ObjectMapper m = new ObjectMapper();
                    JsonNode node = m.readTree(line);
                    ObjectNode response = m.createObjectNode();
                    response.put("status", "ok");
                    response.set("echo", node);
                    OutputStream out = server.getOutputStream();
                    out.write(m.writeValueAsBytes(response));
                    out.write('\n');
                    out.flush();
                } catch (IOException e) {
                    throw new UncheckedIOException(e);
                }
            });
            serverThread.setDaemon(true);
            serverThread.start();

            try (Socket client = new Socket("127.0.0.1", ss.getLocalPort())) {
                NdjsonCodec codec = new NdjsonCodec(client, mapper);

                ObjectNode request = mapper.createObjectNode();
                request.put("type", "CreateNode");
                request.put("id", 42);
                codec.send(request);

                JsonNode response = codec.receive();
                assertThat(response.path("status").asText()).isEqualTo("ok");
                assertThat(response.path("echo").path("type").asText()).isEqualTo("CreateNode");
                assertThat(response.path("echo").path("id").asInt()).isEqualTo(42);
            }
        }
    }
}
