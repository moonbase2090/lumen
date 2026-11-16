Throwaway measurement probe for docs/phase-1/macos-sealing-design.md. It is not product code and is not built by the workspace.
probe.c: the program (modes: info, attack, flags, sock, exec, tiocsti, kq, listen, sleeper). Built with: clang -O1 -o probe probe.c -lbsm
The four targets and the two attackers were the same binary signed four and two ways: ad hoc (plain); ad hoc with --options runtime (hardened); hardened plus the get-task-allow entitlement; hardened plus the com.apple.security.cs.debugger entitlement (attacker).
behaviour-output.txt: audit token across exec, descriptor inheritance, TIOCSTI, kqueue, zsh stream socket, and the first task-port rounds (unsigned-style attacker).
task-port-output.txt: the task-port and ptrace rounds with both attackers. Its standard-user rounds did not run (the account could not be created in the guest) and its later behaviour probes did not run because the session ended; behaviour-output.txt has them.
Machine: clean macOS 26.6.2 arm64 virtual machine, System Integrity Protection enabled, deleted after the run.
