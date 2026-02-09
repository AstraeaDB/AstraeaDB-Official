package com.astraeadb.json;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.io.*;
import java.net.Socket;

/**
 * Internal codec for reading/writing NDJSON (newline-delimited JSON) over a Socket's streams.
 */
class NdjsonCodec {

    private final ObjectMapper mapper;
    private final BufferedReader reader;
    private final OutputStream writer;

    NdjsonCodec(Socket socket, ObjectMapper mapper) throws IOException {
        this.mapper = mapper;
        this.reader = new BufferedReader(new InputStreamReader(socket.getInputStream()));
        this.writer = socket.getOutputStream();
    }

    /**
     * Serializes the given ObjectNode to JSON bytes and writes it as a single NDJSON line.
     */
    void send(ObjectNode request) throws IOException {
        byte[] bytes = mapper.writeValueAsBytes(request);
        writer.write(bytes);
        writer.write('\n');
        writer.flush();
    }

    /**
     * Reads a single NDJSON line from the socket and parses it as a JsonNode.
     *
     * @throws IOException if the connection is closed or the line cannot be parsed
     */
    JsonNode receive() throws IOException {
        String line = reader.readLine();
        if (line == null) {
            throw new IOException("Connection closed by server");
        }
        return mapper.readTree(line);
    }
}
