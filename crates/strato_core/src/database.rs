//! Built-in blocking database seed entries.

use crate::types::{BlockingCategory, BlockingConfig, BlockingDatabase, BlockingEntry};

/// Build the effective blocking database from built-ins plus config overrides.
#[must_use]
pub fn effective_database(config: &BlockingConfig) -> BlockingDatabase {
    let mut database = builtins();

    for removed in &config.remove {
        database.entries.remove(removed);
    }
    for entry in &config.add {
        database.entries.insert(entry.name.clone(), entry.clone());
    }
    database
        .blocking_modules
        .extend(config.blocking_modules.iter().cloned());

    database
}

fn builtins() -> BlockingDatabase {
    let mut database = BlockingDatabase::default();
    for entry in builtin_entries() {
        database.entries.insert(entry.name.clone(), entry);
    }
    database
}

const BUILTIN_ENTRIES: &[(&str, &str, BlockingCategory)] = &[
    ("time.sleep", "Use asyncio.sleep()", BlockingCategory::Sleep),
    (
        "requests.get",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.post",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.put",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.delete",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.patch",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.head",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.options",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.request",
        "Use aiohttp or httpx",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.get",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.post",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.put",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.delete",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.patch",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.head",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.options",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.request",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "requests.Session.send",
        "Use aiohttp.ClientSession",
        BlockingCategory::NetworkIo,
    ),
    (
        "urllib.request.urlopen",
        "Use aiohttp",
        BlockingCategory::NetworkIo,
    ),
    (
        "http.client.HTTPConnection.request",
        "Use aiohttp",
        BlockingCategory::NetworkIo,
    ),
    (
        "http.client.HTTPSConnection.request",
        "Use aiohttp",
        BlockingCategory::NetworkIo,
    ),
    (
        "socket.socket.connect",
        "Use asyncio streams",
        BlockingCategory::NetworkIo,
    ),
    (
        "socket.socket.recv",
        "Use asyncio streams",
        BlockingCategory::NetworkIo,
    ),
    (
        "socket.socket.send",
        "Use asyncio streams",
        BlockingCategory::NetworkIo,
    ),
    (
        "socket.socket.accept",
        "Use asyncio.start_server()",
        BlockingCategory::NetworkIo,
    ),
    (
        "socket.socket.sendall",
        "Use asyncio streams",
        BlockingCategory::NetworkIo,
    ),
    (
        "socket.socket.recvfrom",
        "Use asyncio datagram",
        BlockingCategory::NetworkIo,
    ),
    (
        "socket.create_connection",
        "Use asyncio.open_connection()",
        BlockingCategory::NetworkIo,
    ),
    (
        "builtins.open",
        "Use aiofiles.open()",
        BlockingCategory::FileIo,
    ),
    ("io.open", "Use aiofiles.open()", BlockingCategory::FileIo),
    (
        "os.read",
        "Use aiofiles or run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "os.write",
        "Use aiofiles or run_in_executor",
        BlockingCategory::FileIo,
    ),
    ("os.fdopen", "Use aiofiles", BlockingCategory::FileIo),
    (
        "pathlib.Path.read_text",
        "Use aiofiles",
        BlockingCategory::FileIo,
    ),
    (
        "pathlib.Path.read_bytes",
        "Use aiofiles",
        BlockingCategory::FileIo,
    ),
    (
        "pathlib.Path.write_text",
        "Use aiofiles",
        BlockingCategory::FileIo,
    ),
    (
        "pathlib.Path.write_bytes",
        "Use aiofiles",
        BlockingCategory::FileIo,
    ),
    (
        "os.listdir",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "os.scandir",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    ("os.stat", "Use run_in_executor", BlockingCategory::FileIo),
    (
        "os.path.exists",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "os.path.isfile",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "os.path.isdir",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    ("glob.glob", "Use run_in_executor", BlockingCategory::FileIo),
    (
        "glob.iglob",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "shutil.copy",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "shutil.copytree",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "shutil.move",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "shutil.rmtree",
        "Use run_in_executor",
        BlockingCategory::FileIo,
    ),
    (
        "subprocess.run",
        "Use asyncio.create_subprocess_exec()",
        BlockingCategory::Subprocess,
    ),
    (
        "subprocess.call",
        "Use asyncio.create_subprocess_exec()",
        BlockingCategory::Subprocess,
    ),
    (
        "subprocess.check_call",
        "Use asyncio.create_subprocess_exec()",
        BlockingCategory::Subprocess,
    ),
    (
        "subprocess.check_output",
        "Use asyncio.create_subprocess_exec()",
        BlockingCategory::Subprocess,
    ),
    (
        "subprocess.Popen.wait",
        "Use asyncio.create_subprocess_exec()",
        BlockingCategory::Subprocess,
    ),
    (
        "subprocess.Popen.communicate",
        "Use asyncio.create_subprocess_exec()",
        BlockingCategory::Subprocess,
    ),
    (
        "os.system",
        "Use asyncio.create_subprocess_shell()",
        BlockingCategory::Subprocess,
    ),
    (
        "os.popen",
        "Use asyncio.create_subprocess_shell()",
        BlockingCategory::Subprocess,
    ),
    (
        "psycopg2.connect",
        "Use asyncpg",
        BlockingCategory::DatabaseIo,
    ),
    (
        "sqlite3.connect",
        "Use aiosqlite",
        BlockingCategory::DatabaseIo,
    ),
    (
        "pymysql.connect",
        "Use aiomysql",
        BlockingCategory::DatabaseIo,
    ),
    (
        "builtins.input",
        "Use async input library or run_in_executor",
        BlockingCategory::UserInput,
    ),
];

fn builtin_entries() -> Vec<BlockingEntry> {
    BUILTIN_ENTRIES
        .iter()
        .map(|(name, help, category)| BlockingEntry {
            name: (*name).to_string(),
            help: (*help).to_string(),
            category: *category,
        })
        .collect()
}
