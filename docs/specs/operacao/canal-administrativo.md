# Canal administrativo local — A4

A [P3](diagnostico.md) estende este protocolo com o comando somente de leitura
`diagnose`, sem alterar os limites nem publicar administração no HTTP.
A [A7](servico.md) fornece `RuntimeDirectory`/`TMPDIR` privado no systemd;
o IPC continua acessível somente à conta do serviço/root no Unix. Para operar
o painel após instalar como serviço, use uma dessas identidades do SO.

## 1. Comportamento

1. `serve` cria o endpoint IPC do [ADR 0004](../../adr/0004-canal-administrativo-local.md)
   antes de anunciar prontidão. Permissões não configuráveis nesta etapa:
   proprietário/root no Unix; proprietário, SYSTEM e Administradores no Windows.
2. A metadata tem `admin` opcional com transporte e endereço. Unix anuncia
   `{"transport":"unix","path":"…/elzevir-admin-…/admin.sock"}`; Windows
   anuncia `{"transport":"windows","name":"elzevir-admin-<instance>"}`.
   O cliente valida endereço local e permissões antes de conectar.
3. Pedido: `{"schema":1,"instance":"<id>","id":1,"command":{"type":"status"}}`
   seguido de LF. Resposta: schema, instance, id e result tipado, contendo
   `{"type":"status","health":…}` ou `{"type":"error","code":…}`.
   O id é null quando o pedido não pode ser decodificado.
4. Só há um pedido e uma resposta por conexão. Frames têm no máximo 4096 bytes
   incluindo LF. UTF-8/JSON inválido, campos extras ou comando desconhecido
   resultam em `invalid_request`; schema incorreto dá `unsupported_schema`;
   execução divergente dá `instance_mismatch`. Erros não ecoam conteúdo recebido.
5. O cliente valida schema, id e identidade da resposta e do relatório interno.
   Só considera saudável o relatório real com banco e indexador `ok`.
6. Conexão, pedido e resposta têm prazo global de dois segundos. O servidor
   atende no máximo oito conexões em processamento; excedentes são fechados.
   Clientes lentos não bloqueiam o laço de aceitação nem crescem sem limite.
   A sonda compartilhada admite até quatro leituras simultâneas; a permissão
   continua ocupada pelo worker mesmo que o cliente cancele ou expire.
7. `Status` reutiliza a sonda da A3. Não abre SQLite no cliente, não executa
   migração, não cria dados e não envia comandos de escrita.
8. `status --config ARQUIVO --json` (ou `--data-dir`) prefere o canal quando
   existe metadata IPC. Qualquer falha produz `unavailable`, sem fallback
   silencioso para HTTP. Metadata antiga sem IPC mantém a compatibilidade A3.
9. Ctrl+C encerra o canal e cancela atendimentos pendentes. A pasta/socket
   privados são removidos após fechar o listener. Queda abrupta não impede
   novo início, pois cada execução recebe endereço novo.
10. Mensagens e logs não incluem frames brutos, pedidos recusados ou segredos.
    Saúde pública e permissões do login web não são ampliadas.

## 2. Origem no Koha

Não porta comportamento do Koha. Base: A4 e §4.3 da análise do painel de Paulo.

## 3. Paridade

Livre. `interprocess` implementa IPC Unix/Windows com Tokio; `widestring`
(MIT/Apache-2.0, apenas Windows) codifica a DACL sem chamadas unsafe locais.

## 4. Casos de borda

- Status real funciona com a trava ocupada e confirma a identidade por IPC.
- Pedido inválido, desconhecido, grande ou incompleto não encerra o servidor.
- Versão e identidade divergentes recebem recusa tipada.
- O status não contorna um canal anunciado mas indisponível usando HTTP.
- Unix não segue links nem aceita permissões abertas na pasta ou socket.
- Endereço morto de uma execução anterior não é reutilizado no próximo boot.
- O canal HTTP da A3 e o listener web não oferecem os comandos IPC.

## 5. Desvios

Nenhum: não há área de paridade.

## 6. Fora do escopo

Backup/verify (A5), agendas (A6), serviço/shutdown (A7), reindexação completa,
reload, progresso de trabalhos, grupos configuráveis e interface real P2.
Só o transporte Windows poderá ser conferido por compilação cruzada nesta
máquina; testes de execução Windows dependem de um runner dessa plataforma.
