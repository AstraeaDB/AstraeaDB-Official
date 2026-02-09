package com.astraeadb.exception;

public class AstraeaException extends Exception {
    public AstraeaException(String message) {
        super(message);
    }
    public AstraeaException(String message, Throwable cause) {
        super(message, cause);
    }
}
