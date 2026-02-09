@rem
@rem Copyright 2015 the original author or authors.
@rem
@rem Licensed under the Apache License, Version 2.0 (the "License");
@rem you may not use this file except in compliance with the License.
@rem You may obtain a copy of the License at
@rem
@rem      https://www.apache.org/licenses/LICENSE-2.0
@rem
@rem Unless required by applicable law or agreed to in writing, software
@rem distributed under the License is distributed on an "AS IS" BASIS,
@rem WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
@rem See the License for the specific language governing permissions and
@rem limitations under the License.
@rem
@rem SPDX-License-Identifier: Apache-2.0
@rem

@if "%DEBUG%"=="" @echo off
@rem ##########################################################################
@rem
@rem  examples startup script for Windows
@rem
@rem ##########################################################################

@rem Set local scope for the variables with windows NT shell
if "%OS%"=="Windows_NT" setlocal

set DIRNAME=%~dp0
if "%DIRNAME%"=="" set DIRNAME=.
@rem This is normally unused
set APP_BASE_NAME=%~n0
set APP_HOME=%DIRNAME%..

@rem Resolve any "." and ".." in APP_HOME to make it shorter.
for %%i in ("%APP_HOME%") do set APP_HOME=%%~fi

@rem Add default JVM options here. You can also use JAVA_OPTS and EXAMPLES_OPTS to pass JVM options to this script.
set DEFAULT_JVM_OPTS=

@rem Find java.exe
if defined JAVA_HOME goto findJavaFromJavaHome

set JAVA_EXE=java.exe
%JAVA_EXE% -version >NUL 2>&1
if %ERRORLEVEL% equ 0 goto execute

echo. 1>&2
echo ERROR: JAVA_HOME is not set and no 'java' command could be found in your PATH. 1>&2
echo. 1>&2
echo Please set the JAVA_HOME variable in your environment to match the 1>&2
echo location of your Java installation. 1>&2

goto fail

:findJavaFromJavaHome
set JAVA_HOME=%JAVA_HOME:"=%
set JAVA_EXE=%JAVA_HOME%/bin/java.exe

if exist "%JAVA_EXE%" goto execute

echo. 1>&2
echo ERROR: JAVA_HOME is set to an invalid directory: %JAVA_HOME% 1>&2
echo. 1>&2
echo Please set the JAVA_HOME variable in your environment to match the 1>&2
echo location of your Java installation. 1>&2

goto fail

:execute
@rem Setup the command line

set CLASSPATH=%APP_HOME%\lib\examples-0.1.0.jar;%APP_HOME%\lib\astraeadb-unified-0.1.0.jar;%APP_HOME%\lib\astraeadb-json-0.1.0.jar;%APP_HOME%\lib\astraeadb-grpc-0.1.0.jar;%APP_HOME%\lib\astraeadb-flight-0.1.0.jar;%APP_HOME%\lib\astraeadb-api-0.1.0.jar;%APP_HOME%\lib\flight-core-18.1.0.jar;%APP_HOME%\lib\arrow-vector-18.1.0.jar;%APP_HOME%\lib\jackson-datatype-jsr310-2.18.3.jar;%APP_HOME%\lib\jackson-annotations-2.18.3.jar;%APP_HOME%\lib\jackson-core-2.18.3.jar;%APP_HOME%\lib\jackson-databind-2.18.3.jar;%APP_HOME%\lib\grpc-netty-shaded-1.72.0.jar;%APP_HOME%\lib\grpc-protobuf-1.72.0.jar;%APP_HOME%\lib\grpc-stub-1.72.0.jar;%APP_HOME%\lib\proto-google-common-protos-2.51.0.jar;%APP_HOME%\lib\protobuf-java-util-3.25.4.jar;%APP_HOME%\lib\protobuf-java-3.25.5.jar;%APP_HOME%\lib\arrow-memory-netty-18.1.0.jar;%APP_HOME%\lib\grpc-netty-1.65.0.jar;%APP_HOME%\lib\grpc-util-1.72.0.jar;%APP_HOME%\lib\grpc-core-1.72.0.jar;%APP_HOME%\lib\grpc-protobuf-lite-1.72.0.jar;%APP_HOME%\lib\grpc-context-1.72.0.jar;%APP_HOME%\lib\grpc-api-1.72.0.jar;%APP_HOME%\lib\guava-33.3.1-jre.jar;%APP_HOME%\lib\gson-2.11.0.jar;%APP_HOME%\lib\error_prone_annotations-2.30.0.jar;%APP_HOME%\lib\perfmark-api-0.27.0.jar;%APP_HOME%\lib\animal-sniffer-annotations-1.24.jar;%APP_HOME%\lib\jsr305-3.0.2.jar;%APP_HOME%\lib\arrow-format-18.1.0.jar;%APP_HOME%\lib\arrow-memory-netty-buffer-patch-18.1.0.jar;%APP_HOME%\lib\arrow-memory-core-18.1.0.jar;%APP_HOME%\lib\netty-tcnative-boringssl-static-2.0.66.Final.jar;%APP_HOME%\lib\netty-codec-http2-4.1.100.Final.jar;%APP_HOME%\lib\netty-handler-proxy-4.1.100.Final.jar;%APP_HOME%\lib\netty-codec-http-4.1.100.Final.jar;%APP_HOME%\lib\netty-handler-4.1.114.Final.jar;%APP_HOME%\lib\netty-transport-native-unix-common-4.1.114.Final.jar;%APP_HOME%\lib\netty-codec-socks-4.1.100.Final.jar;%APP_HOME%\lib\netty-codec-4.1.114.Final.jar;%APP_HOME%\lib\netty-transport-4.1.114.Final.jar;%APP_HOME%\lib\netty-buffer-4.1.114.Final.jar;%APP_HOME%\lib\slf4j-api-2.0.16.jar;%APP_HOME%\lib\javax.annotation-api-1.3.2.jar;%APP_HOME%\lib\commons-codec-1.17.1.jar;%APP_HOME%\lib\flatbuffers-java-24.3.25.jar;%APP_HOME%\lib\netty-resolver-4.1.114.Final.jar;%APP_HOME%\lib\netty-common-4.1.114.Final.jar;%APP_HOME%\lib\annotations-4.1.1.4.jar;%APP_HOME%\lib\failureaccess-1.0.2.jar;%APP_HOME%\lib\listenablefuture-9999.0-empty-to-avoid-conflict-with-guava.jar;%APP_HOME%\lib\checker-qual-3.43.0.jar;%APP_HOME%\lib\j2objc-annotations-3.0.0.jar;%APP_HOME%\lib\netty-tcnative-classes-2.0.66.Final.jar


@rem Execute examples
"%JAVA_EXE%" %DEFAULT_JVM_OPTS% %JAVA_OPTS% %EXAMPLES_OPTS%  -classpath "%CLASSPATH%"  %*

:end
@rem End local scope for the variables with windows NT shell
if %ERRORLEVEL% equ 0 goto mainEnd

:fail
rem Set variable EXAMPLES_EXIT_CONSOLE if you need the _script_ return code instead of
rem the _cmd.exe /c_ return code!
set EXIT_CODE=%ERRORLEVEL%
if %EXIT_CODE% equ 0 set EXIT_CODE=1
if not ""=="%EXAMPLES_EXIT_CONSOLE%" exit %EXIT_CODE%
exit /b %EXIT_CODE%

:mainEnd
if "%OS%"=="Windows_NT" endlocal

:omega
