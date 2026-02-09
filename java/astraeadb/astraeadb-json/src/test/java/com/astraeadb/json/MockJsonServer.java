package com.astraeadb.json;

import java.io.*;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;

/**
 * A reusable mock server for testing the JSON/TCP client.
 * <p>
 * Opens a {@link ServerSocket} on an ephemeral port (port 0), accepts one client at a time
 * on a background thread, and replays pre-staged responses.  Captured requests can be
 * inspected via {@link #takeRequest()} after the client has sent a message.
 */
class MockJsonServer implements AutoCloseable {

    private final ServerSocket serverSocket;
    private final BlockingQueue<String> responses = new LinkedBlockingQueue<>();
    private final BlockingQueue<String> requests = new LinkedBlockingQueue<>();
    private final Thread acceptThread;
    private volatile boolean running = true;

    MockJsonServer() throws IOException {
        serverSocket = new ServerSocket(0);
        acceptThread = new Thread(this::acceptLoop, "mock-server");
        acceptThread.setDaemon(true);
        acceptThread.start();
    }

    /** Returns the ephemeral port the server is listening on. */
    int port() {
        return serverSocket.getLocalPort();
    }

    /**
     * Enqueues a raw JSON string that will be sent back to the next client request.
     * Multiple calls stage multiple responses in order.
     */
    void enqueueResponse(String json) {
        responses.add(json);
    }

    /**
     * Blocks until a request has been received from the client and returns the raw NDJSON line.
     *
     * @throws InterruptedException if the wait is interrupted
     */
    String takeRequest() throws InterruptedException {
        return requests.poll(5, TimeUnit.SECONDS);
    }

    @Override
    public void close() throws IOException {
        running = false;
        serverSocket.close();
        acceptThread.interrupt();
    }

    // ---- internal ---------------------------------------------------

    private void acceptLoop() {
        while (running) {
            try (Socket client = serverSocket.accept()) {
                handleClient(client);
            } catch (IOException e) {
                if (running) {
                    // Unexpected – but we keep the loop alive for robustness
                }
            }
        }
    }

    private void handleClient(Socket client) throws IOException {
        BufferedReader reader = new BufferedReader(new InputStreamReader(client.getInputStream()));
        OutputStream writer = client.getOutputStream();

        String line;
        while ((line = reader.readLine()) != null) {
            requests.add(line);
            String response = responses.poll();
            if (response == null) {
                // Default: echo an error so the test knows something is wrong
                response = "{\"status\":\"error\",\"message\":\"No staged response\"}";
            }
            writer.write(response.getBytes());
            writer.write('\n');
            writer.flush();
        }
    }
}
