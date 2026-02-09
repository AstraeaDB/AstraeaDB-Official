package com.astraeadb.exception;

public final class ErrorClassifier {
    private ErrorClassifier() {}

    public static AstraeaException classify(String message) {
        if (message == null) return new AstraeaException("Unknown error");
        String lower = message.toLowerCase();
        if (lower.contains("not found")) {
            if (lower.contains("node")) return new NodeNotFoundException(message);
            if (lower.contains("edge")) return new EdgeNotFoundException(message);
        }
        if (lower.contains("vector index not configured"))
            return new VectorIndexNotConfiguredException(message);
        if (lower.contains("access denied"))
            return new AccessDeniedException(message);
        if (lower.contains("invalid credentials"))
            return new InvalidCredentialsException(message);
        if (lower.contains("authentication required"))
            return new AuthRequiredException(message);
        return new AstraeaException(message);
    }
}
