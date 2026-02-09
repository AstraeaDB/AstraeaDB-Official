dependencies {
    testImplementation("org.junit.jupiter:junit-jupiter:${property("junitVersion")}")
    testImplementation("org.assertj:assertj-core:${property("assertjVersion")}")
    // Jackson only needed for serialization tests
    testImplementation("com.fasterxml.jackson.core:jackson-databind:${property("jacksonVersion")}")
}
