package com.astraeadb.exception;

public class NotConnectedException extends AstraeaException {
    public NotConnectedException() { super("Not connected; call connect() first"); }
}
