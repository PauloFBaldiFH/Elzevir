# ADR 0006: trabalhador operacional e recuperação de arquivos

- Status: adotado na continuidade do roadmap autorizado por Paulo.
- Specs: backup, agendador, painel de operações e distribuição em `docs/specs/operacao`.

O binário compõe as operações em módulos privados e adaptadores testáveis. O
painel recebe projeções e intenções tipadas, sem dependências de store, banco,
gerenciador de serviços ou executores de processos. Isso mantém as fronteiras
existentes sem adicionar uma camada ao núcleo BIBFRAME.

Há um trabalhador por instituição para operações com o servidor ativo. O IPC
aceita identificadores de artefatos, nunca um destino arbitrário para a conta
do serviço. O histórico é limitado e persistido antes de aceitar o trabalho.
Cópias SQLite usam a API de backup a partir de conexão somente de leitura; a
nova conexão de escrita pertence exclusivamente ao arquivo de destino.

O formato de backup é tar/gzip autenticado e cifrado com age/X25519, usando
implementação existente. Configuração e segredos ficam dentro da cifra; a chave
de recuperação fica fora e é exportada somente para arquivo novo privado.
Extração valida manifesto, nomes, tamanhos, hashes e integridade do SQLite.

Restauração mantém o inode da trava e um diário com conjunto fixo de destinos.
Após interrupção, inicialização recusa abrir o banco até recuperar o diário.
Os originais ficam em diretório privado; erros comuns desfazem automaticamente
a troca e a recuperação interrompida pode ser repetida. A versão reconstruída
do índice não é transportada no backup. Permissões e dono da instituição são
preservados em restaurações elevadas.

Operações que param o próprio servidor são executadas por cliente externo com
privilégios do operador e confirmação explícita. Nenhuma integração executa
comandos de shell com valores do usuário. CI usa adaptadores falsos para ações
de sistema e diretórios/bancos fictícios para arquivos.
