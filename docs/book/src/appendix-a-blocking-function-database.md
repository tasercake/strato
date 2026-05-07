# Appendix A: Blocking Function Database (Complete)

This appendix lists every built-in entry in Strato's blocking function database: 61 entries across six categories. Users can extend this via `[tool.strato.blocking]` configuration ([User Configuration](./blocking-function-database-annotations.md#user-configuration)) or `@blocking` decorator ([Annotations API](./blocking-function-database-annotations.md#annotations-api-blocking-non_blocking-unblocker)).

### Sleep

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `time.sleep` | Use `asyncio.sleep()` | Blocks the event loop for the specified duration |

### Network I/O

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `requests.get` | Use `aiohttp` or `httpx` | Synchronous HTTP GET request |
| `requests.post` | Use `aiohttp` or `httpx` | Synchronous HTTP POST request |
| `requests.put` | Use `aiohttp` or `httpx` | Synchronous HTTP PUT request |
| `requests.delete` | Use `aiohttp` or `httpx` | Synchronous HTTP DELETE request |
| `requests.patch` | Use `aiohttp` or `httpx` | Synchronous HTTP PATCH request |
| `requests.head` | Use `aiohttp` or `httpx` | Synchronous HTTP HEAD request |
| `requests.options` | Use `aiohttp` or `httpx` | Synchronous HTTP OPTIONS request |
| `requests.request` | Use `aiohttp` or `httpx` | Generic synchronous HTTP request |
| `requests.Session.get` | Use `aiohttp.ClientSession` | Session-based HTTP GET |
| `requests.Session.post` | Use `aiohttp.ClientSession` | Session-based HTTP POST |
| `requests.Session.put` | Use `aiohttp.ClientSession` | Session-based HTTP PUT |
| `requests.Session.delete` | Use `aiohttp.ClientSession` | Session-based HTTP DELETE |
| `requests.Session.patch` | Use `aiohttp.ClientSession` | Session-based HTTP PATCH |
| `requests.Session.head` | Use `aiohttp.ClientSession` | Session-based HTTP HEAD |
| `requests.Session.options` | Use `aiohttp.ClientSession` | Session-based HTTP OPTIONS |
| `requests.Session.request` | Use `aiohttp.ClientSession` | Generic session-based HTTP request |
| `requests.Session.send` | Use `aiohttp.ClientSession` | Send prepared request via session |
| `urllib.request.urlopen` | Use `aiohttp` | Opens URL and reads response synchronously |
| `http.client.HTTPConnection.request` | Use `aiohttp` | Low-level HTTP connection request |
| `http.client.HTTPSConnection.request` | Use `aiohttp` | Low-level HTTPS connection request |
| `socket.socket.connect` | Use `asyncio` streams | Establishes socket connection |
| `socket.socket.recv` | Use `asyncio` streams | Receives data from socket |
| `socket.socket.send` | Use `asyncio` streams | Sends data through socket |
| `socket.socket.accept` | Use `asyncio.start_server()` | Accepts incoming socket connection |
| `socket.socket.sendall` | Use `asyncio` streams | Sends all data through socket |
| `socket.socket.recvfrom` | Use `asyncio` datagram | Receives data from datagram socket |
| `socket.create_connection` | Use `asyncio.open_connection()` | Creates and connects socket |

### File I/O

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `builtins.open` | Use `aiofiles.open()` | Opens file for reading or writing |
| `io.open` | Use `aiofiles.open()` | Alternative file opening interface |
| `os.read` | Use `aiofiles` or `run_in_executor` | Low-level file descriptor read |
| `os.write` | Use `aiofiles` or `run_in_executor` | Low-level file descriptor write |
| `os.fdopen` | Use `aiofiles` | Opens file descriptor as file object |
| `pathlib.Path.read_text` | Use `aiofiles` | Reads entire file as text |
| `pathlib.Path.read_bytes` | Use `aiofiles` | Reads entire file as bytes |
| `pathlib.Path.write_text` | Use `aiofiles` | Writes text to file |
| `pathlib.Path.write_bytes` | Use `aiofiles` | Writes bytes to file |
| `os.listdir` | Use `run_in_executor` | Lists directory contents |
| `os.scandir` | Use `run_in_executor` | Scans directory with detailed info |
| `os.stat` | Use `run_in_executor` | Gets file status information |
| `os.path.exists` | Use `run_in_executor` | Checks if path exists |
| `os.path.isfile` | Use `run_in_executor` | Checks if path is a file |
| `os.path.isdir` | Use `run_in_executor` | Checks if path is a directory |
| `glob.glob` | Use `run_in_executor` | Finds files matching pattern |
| `glob.iglob` | Use `run_in_executor` | Iterator for files matching pattern |
| `shutil.copy` | Use `run_in_executor` | Copies file |
| `shutil.move` | Use `run_in_executor` | Moves file or directory |
| `shutil.rmtree` | Use `run_in_executor` | Recursively removes directory tree |

### Subprocess

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `subprocess.run` | Use `asyncio.create_subprocess_exec()` | Runs command and waits for completion |
| `subprocess.call` | Use `asyncio.create_subprocess_exec()` | Runs command and returns exit code |
| `subprocess.check_call` | Use `asyncio.create_subprocess_exec()` | Runs command, raises on non-zero exit |
| `subprocess.check_output` | Use `asyncio.create_subprocess_exec()` | Runs command and captures output |
| `subprocess.Popen.wait` | Use `asyncio.create_subprocess_exec()` | Waits for subprocess to complete |
| `subprocess.Popen.communicate` | Use `asyncio.create_subprocess_exec()` | Sends input and reads output from subprocess |
| `os.system` | Use `asyncio.create_subprocess_shell()` | Executes shell command |
| `os.popen` | Use `asyncio.create_subprocess_shell()` | Opens pipe to/from shell command |

### Database

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `psycopg2.connect` | Use `asyncpg` | Establishes PostgreSQL connection |
| `sqlite3.connect` | Use `aiosqlite` | Establishes SQLite connection |
| `pymysql.connect` | Use `aiomysql` | Establishes MySQL connection |

### User Input

| Qualified Name | Help Text | Notes |
|----------------|-----------|-------|
| `builtins.input` | Use async input library or `run_in_executor` | Waits for user input from stdin |
