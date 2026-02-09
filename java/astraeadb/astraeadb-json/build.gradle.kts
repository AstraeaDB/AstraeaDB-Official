dependencies {
    api(project(":astraeadb-api"))
    implementation("com.fasterxml.jackson.core:jackson-databind:${property("jacksonVersion")}")

    testImplementation("org.junit.jupiter:junit-jupiter:${property("junitVersion")}")
    testImplementation("org.assertj:assertj-core:${property("assertjVersion")}")
}
