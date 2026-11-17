# ADR 0004: canal administrativo local com permissões do sistema

- **Status:** adotado na execução da A4, seguindo a alternativa recomendada na
  análise do painel fornecida por Paulo. A continuidade da implementação foi autorizada.
- **Spec:** [canal administrativo](../specs/operacao/canal-administrativo.md).

## Contexto

O servidor mantém a trava exclusiva da instituição. Painel e CLI não podem
abrir uma segunda escritora para administrar um servidor ativo. O endpoint
HTTP da A3 é uma sonda de saúde, sem autorização para comandos.

## Decisão

1. IPC nativo, sem porta TCP administrativa: socket Unix em pasta temporária
   privada (0700; socket 0600), named pipe local no Windows. Usamos a API
   segura de `interprocess` (Apache-2.0/0BSD), sem unsafe no Elzevir.
2. Windows: DACL protegida com acesso total apenas a SYSTEM, Administradores
   e proprietário do objeto (Owner Rights). Clientes remotos são recusados
   pelo backend. Unix: cliente exige pasta e socket privados, sem links,
   pertencentes ao usuário efetivo ou acessíveis por root.
3. O endereço é exclusivo por execução, descoberto por um campo opcional
   `admin` em `elzevir-runtime.json`. Não há nome global compartilhado por
   instituições. A pasta temporária reduz o caminho Unix, que ainda precisa
   caber no limite de tamanho de endereço do SO;
   configuração de diretório de runtime de serviço fica para A7.
4. Uma linha JSON por conexão, com schema, identidade da execução, id numérico
   e comando tipado. Respostas ecoam id e identidade. A4 oferece somente
   `Status`; não oferece SQL, caminhos arbitrários, shell, backup ou shutdown.
5. O servidor despacha operações, sem duplicar regra de negócio. Escritas futuras
   devem usar os comandos da escritora; trabalhos demorados precisam de workers.
6. O `status` usa IPC quando anunciado. Falha de IPC não faz fallback para HTTP.
   Metadata antiga da A3 sem `admin` continua usando somente a sonda HTTP.

## Consequências

- Autorização local é pelo sistema operacional, separada do login web.
  Não há token em variável, arquivo ou parâmetro do canal.
- No serviço Windows, um administrador pode precisar de terminal elevado.
  Grupo Unix administrativo e ACLs da conta de serviço serão tratados na A7.
- Fila de conexões e frames são limitados; tempo limite também vale para
  clientes que não terminam a linha. Não se promete progresso de jobs nesta etapa.
- Na parada normal, tarefas e endpoint são encerrados e a pasta privada removida.
  Após kill/crash pode restar pasta temporária; nova execução não a reutiliza nem
  remove caminhos antigos às cegas.
- A3 continua com listener de saúde separado. A4 não publica comandos no HTTP.
- Os transitivos `recvmsg` 1.0.0 e `doctest-file` 1.1.1 usam 0BSD. Seus
  LICENSE.txt foram conferidos: permissão irrestrita de uso, cópia, modificação
  e distribuição, sem condição incompatível com GPL. `deny.toml` aceita essa
  licença só para essas versões, sem ampliar a lista global de licenças.
- Metadata IPC não é uma identidade autenticada contra um usuário que já controla
  os arquivos/conta do servidor; protege descoberta acidental de outra execução.

## Alternativas

- HTTP local com token: exigiria gerenciar segredo e uma porta privilegiada
  de administração; dispensável com os transportes locais suportados.
- Pasta fixa global `/run/elzevir`: exige provisionamento e permissões do
  serviço, ainda não existentes. A7 poderá definir uma pasta compartilhada.
- Abrir SQLite pelo painel: viola o dono único da escrita e a trava existente.
