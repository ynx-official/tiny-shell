use std::collections::HashSet;

use super::config::{QuickCommand, QuickCommandCategory};

pub(crate) const BUILTIN_QUICK_COMMANDS_VERSION: u32 = 1;

struct CommandDefinition {
    id: &'static str,
    zh_name: &'static str,
    en_name: &'static str,
    command: &'static str,
    zh_remark: &'static str,
    en_remark: &'static str,
}

struct CategoryDefinition {
    id: &'static str,
    zh_name: &'static str,
    en_name: &'static str,
    commands: &'static [CommandDefinition],
}

macro_rules! command {
    ($id:literal, $zh_name:literal, $en_name:literal, $command:literal, $zh_remark:literal, $en_remark:literal) => {
        CommandDefinition {
            id: $id,
            zh_name: $zh_name,
            en_name: $en_name,
            command: $command,
            zh_remark: $zh_remark,
            en_remark: $en_remark,
        }
    };
}

const FILE_COMMANDS: &[CommandDefinition] = &[
    command!(
        "ls",
        "列出目录",
        "List Directory",
        "ls",
        "列出当前目录内容",
        "List the current directory"
    ),
    command!(
        "ls-long",
        "文件详情",
        "File Details",
        "ls -l",
        "以长格式显示权限、大小和时间",
        "Show permissions, size, and time in long format"
    ),
    command!(
        "ls-all",
        "显示隐藏",
        "Show Hidden",
        "ls -a",
        "显示包括隐藏文件在内的所有文件",
        "Include hidden files"
    ),
    command!(
        "ls-human",
        "友好大小",
        "Readable Sizes",
        "ls -h",
        "以 K、M、G 等单位显示文件大小",
        "Show sizes using K, M, and G units"
    ),
    command!(
        "cd",
        "切换目录",
        "Change Directory",
        "cd",
        "切换工作目录",
        "Change the working directory"
    ),
    command!(
        "cd-parent",
        "返回上级",
        "Parent Directory",
        "cd ..",
        "切换到上一级目录",
        "Change to the parent directory"
    ),
    command!(
        "cd-home",
        "用户目录",
        "Home Directory",
        "cd ~",
        "切换到当前用户家目录",
        "Change to the current user's home directory"
    ),
    command!(
        "cd-previous",
        "上一目录",
        "Previous Directory",
        "cd -",
        "切换回上一次所在目录",
        "Return to the previous directory"
    ),
    command!(
        "pwd",
        "当前路径",
        "Current Path",
        "pwd",
        "打印当前工作目录的绝对路径",
        "Print the absolute path of the working directory"
    ),
    command!(
        "mkdir",
        "创建目录",
        "Create Directory",
        "mkdir",
        "创建一个新目录",
        "Create a directory"
    ),
    command!(
        "mkdir-parents",
        "递归建目录",
        "Create Parent Paths",
        "mkdir -p",
        "递归创建多层目录",
        "Create parent directories as needed"
    ),
    command!(
        "rmdir",
        "删除空目录",
        "Remove Empty Directory",
        "rmdir",
        "删除空目录",
        "Remove an empty directory"
    ),
    command!(
        "touch",
        "创建空文件",
        "Create Empty File",
        "touch",
        "创建空文件或更新时间戳",
        "Create an empty file or update timestamps"
    ),
    command!(
        "cp",
        "复制文件",
        "Copy File",
        "cp",
        "复制文件",
        "Copy a file"
    ),
    command!(
        "cp-recursive",
        "复制目录",
        "Copy Directory",
        "cp -r",
        "递归复制整个目录",
        "Copy a directory recursively"
    ),
    command!(
        "mv",
        "移动重命名",
        "Move or Rename",
        "mv",
        "移动或重命名文件和目录",
        "Move or rename files and directories"
    ),
    command!(
        "rm",
        "删除文件",
        "Remove File",
        "rm",
        "删除文件",
        "Remove a file"
    ),
    command!(
        "rm-recursive",
        "递归删除",
        "Remove Recursively",
        "rm -r",
        "递归删除目录及其内容",
        "Remove a directory and its contents recursively"
    ),
    command!(
        "rm-force",
        "强制删除",
        "Force Remove",
        "rm -f",
        "强制删除且不弹出确认",
        "Force removal without confirmation"
    ),
    command!(
        "ln",
        "创建硬链接",
        "Create Hard Link",
        "ln",
        "创建硬链接",
        "Create a hard link"
    ),
    command!(
        "ln-symbolic",
        "创建软链接",
        "Create Symbolic Link",
        "ln -s",
        "创建符号链接",
        "Create a symbolic link"
    ),
];

const VIEW_COMMANDS: &[CommandDefinition] = &[
    command!(
        "cat",
        "查看全文",
        "Show Full File",
        "cat",
        "一次性输出文件全部内容",
        "Print the entire file"
    ),
    command!(
        "less",
        "分页查看",
        "Paged Viewer",
        "less",
        "分页查看文件，可上下翻页并按 q 退出",
        "View a file page by page; press q to quit"
    ),
    command!(
        "more",
        "向下分页",
        "Forward Pager",
        "more",
        "分页查看文件，只能向下翻页",
        "View a file page by page moving forward"
    ),
    command!(
        "head",
        "查看开头",
        "Show Beginning",
        "head",
        "查看文件开头几行",
        "Show the beginning of a file"
    ),
    command!(
        "head-lines",
        "指定前几行",
        "First N Lines",
        "head -n",
        "指定查看文件前 N 行",
        "Show the first N lines"
    ),
    command!(
        "tail",
        "查看末尾",
        "Show End",
        "tail",
        "查看文件末尾几行",
        "Show the end of a file"
    ),
    command!(
        "tail-lines",
        "指定末几行",
        "Last N Lines",
        "tail -n",
        "指定查看文件末尾 N 行",
        "Show the last N lines"
    ),
    command!(
        "tail-follow",
        "追踪日志",
        "Follow Log",
        "tail -f",
        "实时追踪文件新增内容",
        "Follow newly appended file content"
    ),
    command!(
        "wc-lines",
        "统计行数",
        "Count Lines",
        "wc -l",
        "统计文件行数",
        "Count file lines"
    ),
    command!(
        "file",
        "识别类型",
        "Identify File",
        "file",
        "根据内容识别文件类型",
        "Identify a file type from its content"
    ),
    command!(
        "stat",
        "文件元数据",
        "File Metadata",
        "stat",
        "查看 inode、权限和时间等元数据",
        "Show inode, permissions, timestamps, and other metadata"
    ),
];

const SEARCH_COMMANDS: &[CommandDefinition] = &[
    command!(
        "find",
        "递归查找",
        "Find Files",
        "find",
        "按条件递归查找文件",
        "Find files recursively by conditions"
    ),
    command!(
        "find-name",
        "按名称查找",
        "Find by Name",
        "find -name",
        "按文件名模式查找",
        "Find files by name pattern"
    ),
    command!(
        "find-type",
        "按类型查找",
        "Find by Type",
        "find -type",
        "按文件或目录类型查找",
        "Find by file or directory type"
    ),
    command!(
        "locate",
        "快速定位",
        "Locate Files",
        "locate",
        "基于索引数据库快速查找文件",
        "Find files quickly using an index database"
    ),
    command!(
        "grep",
        "搜索文本",
        "Search Text",
        "grep",
        "在文件中搜索指定文本",
        "Search for text in files"
    ),
    command!(
        "grep-recursive",
        "递归搜索",
        "Search Recursively",
        "grep -r",
        "递归搜索目录中的文本",
        "Search text recursively in a directory"
    ),
    command!(
        "grep-ignore-case",
        "忽略大小写",
        "Ignore Case",
        "grep -i",
        "忽略大小写搜索文本",
        "Search text case-insensitively"
    ),
    command!(
        "grep-line-number",
        "显示行号",
        "Show Line Numbers",
        "grep -n",
        "显示匹配内容所在行号",
        "Show line numbers for matches"
    ),
    command!(
        "grep-invert",
        "反向匹配",
        "Invert Match",
        "grep -v",
        "排除包含指定关键字的行",
        "Exclude lines containing the pattern"
    ),
    command!(
        "sed",
        "流式编辑",
        "Stream Edit",
        "sed",
        "对文本执行替换、增加或删除",
        "Transform text with replacements, insertions, or deletions"
    ),
    command!(
        "sed-in-place",
        "原地替换",
        "Edit In Place",
        "sed -i",
        "直接修改文件内容",
        "Modify file content in place"
    ),
    command!(
        "awk",
        "按列处理",
        "Process Columns",
        "awk",
        "按列处理文本并进行统计",
        "Process and summarize column-based text"
    ),
    command!(
        "sort",
        "文本排序",
        "Sort Lines",
        "sort",
        "对文本行排序",
        "Sort text lines"
    ),
    command!(
        "uniq",
        "相邻去重",
        "Remove Duplicates",
        "uniq",
        "去除相邻的重复行",
        "Remove adjacent duplicate lines"
    ),
    command!(
        "cut",
        "截取列",
        "Extract Columns",
        "cut",
        "按分隔符截取指定列",
        "Extract selected fields by delimiter"
    ),
    command!(
        "tr",
        "字符转换",
        "Translate Characters",
        "tr",
        "替换或转换字符",
        "Replace or translate characters"
    ),
    command!(
        "diff",
        "对比文件",
        "Compare Files",
        "diff",
        "对比两个文件的差异",
        "Compare differences between two files"
    ),
    command!(
        "tee",
        "输出并写入",
        "Display and Write",
        "tee",
        "同时输出到屏幕并写入文件",
        "Display output while writing it to a file"
    ),
    command!(
        "xargs",
        "转换为参数",
        "Build Arguments",
        "xargs",
        "将标准输入转换为后续命令参数",
        "Build command arguments from standard input"
    ),
];

const PERMISSION_COMMANDS: &[CommandDefinition] = &[
    command!(
        "chmod",
        "修改权限",
        "Change Permissions",
        "chmod",
        "修改文件或目录权限",
        "Change file or directory permissions"
    ),
    command!(
        "chmod-755",
        "设置 755",
        "Set 755",
        "chmod 755",
        "设置权限为 rwxr-xr-x",
        "Set permissions to rwxr-xr-x"
    ),
    command!(
        "chmod-executable",
        "添加执行权限",
        "Make Executable",
        "chmod +x",
        "添加可执行权限",
        "Add executable permission"
    ),
    command!(
        "chown",
        "修改所有者",
        "Change Owner",
        "chown",
        "修改文件所有者和所属组",
        "Change file owner and group"
    ),
    command!(
        "chgrp",
        "修改所属组",
        "Change Group",
        "chgrp",
        "修改文件所属组",
        "Change file group"
    ),
    command!(
        "chattr",
        "特殊属性",
        "Special Attributes",
        "chattr",
        "设置不可修改等特殊文件属性",
        "Set special file attributes such as immutable"
    ),
    command!(
        "umask",
        "默认权限掩码",
        "Permission Mask",
        "umask",
        "设置新建文件的默认权限掩码",
        "Set the default permission mask for new files"
    ),
    command!(
        "sudo",
        "管理员执行",
        "Run as Root",
        "sudo",
        "以 root 权限执行命令",
        "Run a command with root privileges"
    ),
    command!(
        "su",
        "切换用户",
        "Switch User",
        "su",
        "切换到其他用户",
        "Switch to another user"
    ),
    command!(
        "useradd",
        "新建用户",
        "Add User",
        "useradd",
        "新建系统用户",
        "Create a system user"
    ),
    command!(
        "userdel",
        "删除用户",
        "Delete User",
        "userdel",
        "删除系统用户",
        "Delete a system user"
    ),
    command!(
        "passwd",
        "修改密码",
        "Change Password",
        "passwd",
        "修改用户密码",
        "Change a user password"
    ),
    command!(
        "usermod",
        "修改用户",
        "Modify User",
        "usermod",
        "修改用户属性或附加组",
        "Modify user properties or supplementary groups"
    ),
    command!(
        "groupadd",
        "新建用户组",
        "Add Group",
        "groupadd",
        "新建用户组",
        "Create a user group"
    ),
    command!(
        "groupdel",
        "删除用户组",
        "Delete Group",
        "groupdel",
        "删除用户组",
        "Delete a user group"
    ),
    command!(
        "id",
        "用户标识",
        "User IDs",
        "id",
        "查看当前用户的 UID 和 GID",
        "Show the current user's UID and GID"
    ),
    command!(
        "whoami",
        "当前用户名",
        "Current User",
        "whoami",
        "查看当前登录用户名",
        "Show the current login username"
    ),
    command!(
        "who",
        "登录用户",
        "Logged-in Users",
        "who",
        "查看当前登录的所有用户",
        "Show all logged-in users"
    ),
];

const SYSTEM_COMMANDS: &[CommandDefinition] = &[
    command!(
        "ps",
        "当前进程",
        "Current Processes",
        "ps",
        "查看当前进程",
        "Show current processes"
    ),
    command!(
        "ps-aux",
        "全部进程",
        "All Processes",
        "ps aux",
        "查看系统所有进程",
        "Show all system processes"
    ),
    command!(
        "top",
        "实时监控",
        "Live Monitor",
        "top",
        "实时监控进程与资源",
        "Monitor processes and resources in real time"
    ),
    command!(
        "htop",
        "交互监控",
        "Interactive Monitor",
        "htop",
        "使用彩色交互界面监控进程",
        "Monitor processes with an interactive interface"
    ),
    command!(
        "kill",
        "终止进程",
        "Terminate Process",
        "kill",
        "按 PID 终止进程",
        "Terminate a process by PID"
    ),
    command!(
        "kill-force",
        "强制杀进程",
        "Force Kill",
        "kill -9",
        "按 PID 强制终止进程",
        "Forcefully terminate a process by PID"
    ),
    command!(
        "pkill",
        "按名称终止",
        "Kill by Name",
        "pkill",
        "按进程名终止进程",
        "Terminate processes by name"
    ),
    command!(
        "killall",
        "终止同名进程",
        "Kill All by Name",
        "killall",
        "终止所有同名进程",
        "Terminate all processes with the same name"
    ),
    command!(
        "jobs",
        "后台任务",
        "Background Jobs",
        "jobs",
        "查看当前 shell 的后台任务",
        "Show background jobs in the current shell"
    ),
    command!(
        "fg",
        "调到前台",
        "Bring to Foreground",
        "fg",
        "将后台任务调到前台",
        "Bring a background job to the foreground"
    ),
    command!(
        "bg",
        "后台继续",
        "Resume in Background",
        "bg",
        "让暂停的任务在后台继续",
        "Resume a stopped job in the background"
    ),
    command!(
        "nohup",
        "后台常驻",
        "Keep Running",
        "nohup",
        "让命令忽略挂断并继续运行",
        "Keep a command running after hangup"
    ),
    command!(
        "free-human",
        "内存使用",
        "Memory Usage",
        "free -h",
        "以友好单位查看内存使用情况",
        "Show memory usage in readable units"
    ),
    command!(
        "df-human",
        "磁盘空间",
        "Disk Space",
        "df -h",
        "查看文件系统剩余空间",
        "Show filesystem space in readable units"
    ),
    command!(
        "du-summary",
        "目录总大小",
        "Directory Size",
        "du -sh",
        "查看目录总大小",
        "Show the total size of a directory"
    ),
    command!(
        "du-depth-one",
        "子目录大小",
        "Subdirectory Sizes",
        "du -h --max-depth=1",
        "查看当前各子目录大小",
        "Show sizes of immediate subdirectories"
    ),
    command!(
        "uptime",
        "运行与负载",
        "Uptime and Load",
        "uptime",
        "查看系统运行时长与负载",
        "Show system uptime and load"
    ),
    command!(
        "uname-all",
        "内核信息",
        "Kernel Info",
        "uname -a",
        "查看内核与系统信息",
        "Show kernel and system information"
    ),
    command!(
        "hostname",
        "主机名",
        "Hostname",
        "hostname",
        "查看或设置主机名",
        "Show or set the hostname"
    ),
    command!(
        "lscpu",
        "CPU 信息",
        "CPU Info",
        "lscpu",
        "查看 CPU 架构与核心信息",
        "Show CPU architecture and core information"
    ),
    command!(
        "systemctl-status",
        "服务状态",
        "Service Status",
        "systemctl status",
        "查看 systemd 服务状态",
        "Show systemd service status"
    ),
    command!(
        "systemctl-start",
        "启动服务",
        "Start Service",
        "systemctl start",
        "启动 systemd 服务",
        "Start a systemd service"
    ),
    command!(
        "systemctl-restart",
        "重启服务",
        "Restart Service",
        "systemctl restart",
        "重启 systemd 服务",
        "Restart a systemd service"
    ),
    command!(
        "systemctl-enable",
        "服务自启",
        "Enable Service",
        "systemctl enable",
        "设置 systemd 服务开机自启",
        "Enable a systemd service at boot"
    ),
    command!(
        "journalctl",
        "系统日志",
        "System Logs",
        "journalctl",
        "查看 systemd 系统日志",
        "View systemd journal logs"
    ),
    command!(
        "history",
        "命令历史",
        "Command History",
        "history",
        "查看历史执行过的命令",
        "Show command history"
    ),
];

const NETWORK_COMMANDS: &[CommandDefinition] = &[
    command!(
        "ip-address",
        "网卡地址",
        "Interface Addresses",
        "ip addr",
        "查看网卡与 IP 地址",
        "Show network interfaces and IP addresses"
    ),
    command!(
        "ifconfig",
        "旧版网卡信息",
        "Legacy Interface Info",
        "ifconfig",
        "使用旧版工具查看网络接口",
        "Show network interfaces using the legacy tool"
    ),
    command!(
        "ping",
        "测试连通性",
        "Test Connectivity",
        "ping",
        "测试到目标主机的网络连通性",
        "Test connectivity to a host"
    ),
    command!(
        "curl",
        "HTTP 请求",
        "HTTP Request",
        "curl",
        "发起 HTTP 请求或传输数据",
        "Make HTTP requests or transfer data"
    ),
    command!(
        "wget",
        "下载文件",
        "Download File",
        "wget",
        "从网络下载文件",
        "Download files from the network"
    ),
    command!(
        "ss-listen",
        "监听端口",
        "Listening Ports",
        "ss -tulnp",
        "查看监听端口及对应进程",
        "Show listening ports and processes"
    ),
    command!(
        "netstat-listen",
        "旧版端口查看",
        "Legacy Port View",
        "netstat -tulnp",
        "使用旧版工具查看连接和监听端口",
        "Show connections and ports using the legacy tool"
    ),
    command!(
        "lsof-port",
        "端口占用",
        "Port Owner",
        "lsof -i:端口",
        "查看指定端口被哪个进程占用",
        "Show which process is using a port"
    ),
    command!(
        "dig",
        "DNS 查询",
        "DNS Query",
        "dig",
        "查询 DNS 解析记录",
        "Query DNS records"
    ),
    command!(
        "nslookup",
        "域名解析",
        "Resolve Domain",
        "nslookup",
        "查询域名对应的 IP 地址",
        "Resolve a domain name to an IP address"
    ),
    command!(
        "ssh",
        "远程登录",
        "Remote Login",
        "ssh",
        "通过 SSH 登录远程服务器",
        "Log in to a remote server over SSH"
    ),
    command!(
        "scp",
        "远程复制",
        "Secure Copy",
        "scp",
        "通过 SSH 在本地与远程间复制文件",
        "Copy files between local and remote hosts over SSH"
    ),
    command!(
        "rsync",
        "增量同步",
        "Incremental Sync",
        "rsync",
        "增量同步文件或目录",
        "Synchronize files or directories incrementally"
    ),
    command!(
        "nc",
        "网络调试",
        "Network Debug",
        "nc",
        "使用 Netcat 调试网络和探测端口",
        "Debug networks and probe ports with Netcat"
    ),
];

const ARCHIVE_COMMANDS: &[CommandDefinition] = &[
    command!(
        "tar",
        "打包解包",
        "Archive Files",
        "tar",
        "打包或解包文件",
        "Create or extract archives"
    ),
    command!(
        "tar-create-gzip",
        "压缩 tar.gz",
        "Create tar.gz",
        "tar -czvf",
        "打包并使用 gzip 压缩",
        "Create a gzip-compressed tar archive"
    ),
    command!(
        "tar-extract-gzip",
        "解压 tar.gz",
        "Extract tar.gz",
        "tar -xzvf",
        "解压 gzip 格式的 tar 包",
        "Extract a gzip-compressed tar archive"
    ),
    command!(
        "gzip",
        "Gzip 压缩",
        "Gzip Compress",
        "gzip",
        "压缩单个文件",
        "Compress a single file with gzip"
    ),
    command!(
        "gunzip",
        "Gzip 解压",
        "Gzip Extract",
        "gunzip",
        "解压 gzip 文件",
        "Extract a gzip file"
    ),
    command!(
        "zip-recursive",
        "压缩 ZIP",
        "Create ZIP",
        "zip -r",
        "将目录递归压缩为 ZIP",
        "Compress a directory recursively as ZIP"
    ),
    command!(
        "unzip",
        "解压 ZIP",
        "Extract ZIP",
        "unzip",
        "解压 ZIP 文件",
        "Extract a ZIP archive"
    ),
];

const UTILITY_COMMANDS: &[CommandDefinition] = &[
    command!(
        "alias",
        "命令别名",
        "Command Alias",
        "alias",
        "为长命令设置别名",
        "Create aliases for long commands"
    ),
    command!(
        "which",
        "命令路径",
        "Command Path",
        "which",
        "查找命令的可执行文件路径",
        "Locate a command executable"
    ),
    command!(
        "whereis",
        "查找命令文件",
        "Find Command Files",
        "whereis",
        "查找命令的二进制、源码和手册",
        "Locate command binaries, source, and manuals"
    ),
    command!(
        "man",
        "命令手册",
        "Command Manual",
        "man",
        "查看命令的完整手册",
        "View a command manual"
    ),
    command!(
        "echo",
        "输出文本",
        "Print Text",
        "echo",
        "输出文本或变量内容",
        "Print text or variable values"
    ),
    command!(
        "env",
        "环境变量",
        "Environment Variables",
        "env",
        "查看环境变量",
        "Show environment variables"
    ),
    command!(
        "export",
        "导出变量",
        "Export Variable",
        "export",
        "设置并导出环境变量",
        "Set and export an environment variable"
    ),
    command!(
        "watch",
        "定时执行",
        "Repeat Command",
        "watch",
        "定时重复执行命令",
        "Run a command repeatedly at intervals"
    ),
    command!(
        "crontab-edit",
        "编辑定时任务",
        "Edit Cron Jobs",
        "crontab -e",
        "编辑当前用户的定时任务",
        "Edit scheduled jobs for the current user"
    ),
];

const CATEGORIES: &[CategoryDefinition] = &[
    CategoryDefinition {
        id: "builtin-files",
        zh_name: "文件与目录",
        en_name: "Files & Directories",
        commands: FILE_COMMANDS,
    },
    CategoryDefinition {
        id: "builtin-view",
        zh_name: "查看文件",
        en_name: "File Viewing",
        commands: VIEW_COMMANDS,
    },
    CategoryDefinition {
        id: "builtin-search",
        zh_name: "查找与文本",
        en_name: "Search & Text",
        commands: SEARCH_COMMANDS,
    },
    CategoryDefinition {
        id: "builtin-permissions",
        zh_name: "权限与用户",
        en_name: "Permissions & Users",
        commands: PERMISSION_COMMANDS,
    },
    CategoryDefinition {
        id: "builtin-system",
        zh_name: "系统与进程",
        en_name: "System & Processes",
        commands: SYSTEM_COMMANDS,
    },
    CategoryDefinition {
        id: "builtin-network",
        zh_name: "网络与远程",
        en_name: "Network & Remote",
        commands: NETWORK_COMMANDS,
    },
    CategoryDefinition {
        id: "builtin-archives",
        zh_name: "压缩与解压",
        en_name: "Compression & Archives",
        commands: ARCHIVE_COMMANDS,
    },
    CategoryDefinition {
        id: "builtin-utilities",
        zh_name: "效率工具",
        en_name: "Utilities",
        commands: UTILITY_COMMANDS,
    },
];

pub(crate) fn builtin_quick_command_categories(locale: &str) -> Vec<QuickCommandCategory> {
    let use_chinese = locale.starts_with("zh");
    CATEGORIES
        .iter()
        .map(|category| QuickCommandCategory {
            id: category.id.to_string(),
            name: localized(use_chinese, category.zh_name, category.en_name).to_string(),
            commands: category
                .commands
                .iter()
                .map(|command| QuickCommand {
                    id: format!("{}-{}", category.id, command.id),
                    name: localized(use_chinese, command.zh_name, command.en_name).to_string(),
                    remark: localized(use_chinese, command.zh_remark, command.en_remark)
                        .to_string(),
                    command: command.command.to_string(),
                })
                .collect(),
        })
        .collect()
}

pub(crate) fn merge_builtin_quick_commands(
    categories: &mut Vec<QuickCommandCategory>,
    locale: &str,
) -> bool {
    let mut existing_commands = categories
        .iter()
        .flat_map(|category| category.commands.iter())
        .map(|command| command.command.trim().to_string())
        .collect::<HashSet<_>>();
    let mut changed = false;

    for mut builtin_category in builtin_quick_command_categories(locale) {
        builtin_category
            .commands
            .retain(|command| existing_commands.insert(command.command.trim().to_string()));
        if builtin_category.commands.is_empty() {
            continue;
        }

        if let Some(existing_category) = categories
            .iter_mut()
            .find(|category| category.id == builtin_category.id)
        {
            existing_category.commands.extend(builtin_category.commands);
        } else {
            categories.push(builtin_category);
        }
        changed = true;
    }

    changed
}

fn localized<'a>(use_chinese: bool, zh: &'a str, en: &'a str) -> &'a str {
    if use_chinese { zh } else { en }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_all_builtin_categories_with_stable_unique_ids() {
        let categories = builtin_quick_command_categories("zh-CN");
        let category_ids = categories
            .iter()
            .map(|category| category.id.as_str())
            .collect::<HashSet<_>>();
        let command_ids = categories
            .iter()
            .flat_map(|category| category.commands.iter())
            .map(|command| command.id.as_str())
            .collect::<HashSet<_>>();
        let command_count = categories
            .iter()
            .map(|category| category.commands.len())
            .sum::<usize>();

        assert_eq!(categories.len(), 8);
        assert_eq!(category_ids.len(), categories.len());
        assert_eq!(command_ids.len(), command_count);
        assert_eq!(command_count, 125);
        assert_eq!(categories[0].commands[0].name, "列出目录");
        assert_eq!(categories[0].commands[0].command, "ls");
    }

    #[test]
    fn uses_english_names_and_remarks_for_non_chinese_locale() {
        let categories = builtin_quick_command_categories("en");

        assert_eq!(categories[0].name, "Files & Directories");
        assert_eq!(categories[0].commands[0].name, "List Directory");
        assert_eq!(
            categories[0].commands[0].remark,
            "List the current directory"
        );
    }

    #[test]
    fn merge_preserves_custom_data_and_deduplicates_by_command_content() {
        let mut categories = vec![QuickCommandCategory {
            id: "custom".into(),
            name: "我的命令".into(),
            commands: vec![QuickCommand {
                id: "custom-ls".into(),
                name: "我的列表".into(),
                remark: "保留用户定义".into(),
                command: "ls".into(),
            }],
        }];

        assert!(merge_builtin_quick_commands(&mut categories, "zh-CN"));
        assert_eq!(categories[0].commands[0].name, "我的列表");
        assert_eq!(
            categories
                .iter()
                .flat_map(|category| category.commands.iter())
                .filter(|command| command.command == "ls")
                .count(),
            1
        );
        assert!(!merge_builtin_quick_commands(&mut categories, "zh-CN"));
    }
}
