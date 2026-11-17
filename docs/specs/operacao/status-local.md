# Descoberta e consulta de estado local — A3.2

## Comportamento

1. `serve` publica `DATA_DIR/elzevir-runtime.json` depois de vincular os dois
   listeners e antes de anunciar o início. O documento tem schema 1, identidade
   aleatória da execução, PID e endereço do listener de saúde em loopback.
   O arquivo temporário é sincronizado e publicado por substituição atômica;
   em Unix nasce com modo 0600. Conteúdo preexistente desconhecido ou link
   simbólico é recusado, preservando o arquivo.
2. A publicação exige a trava da pasta de dados já adquirida por `serve`.
   Na parada normal, remove somente metadados com a identidade desta execução;
   falhas de limpeza são informadas. Queda abrupta pode deixar metadados antigos.
3. `elzevir status --data-dir PASTA` ou `--config ARQUIVO` consulta a trava
   existente e a saúde local, sem abrir SQLite, criar pastas ou gravar arquivos.
   A CLI sobrepõe o arquivo e resolve caminhos como na A1. `--json` produz
   schema 1, state, health opcional e reason opcional; sem a opção, resumo textual.
4. Trava livre significa `stopped`, mesmo com metadados antigos. Trava ocupada
   exige resposta de saúde com schema, identidade e PID correspondentes aos
   metadados. Só resposta 200 com componentes prontos significa `running`;
   resposta 503 com componentes não prontos significa `degraded`.
5. Ausência, erro de acesso, arquivos inválidos, tipo de arquivo inadequado,
   protocolo inconsistente ou saúde inacessível significam `unavailable`.
   Somente `running` retorna código de saída zero. Falhas de configuração ou
   argumentos continuam sendo erros de CLI, sem emitir um documento de estado.
6. Metadados precisam ser arquivo regular, têm limite de 4 KiB e aceitam
   apenas schema 1, PID não zero, identidade hexadecimal de 32 caracteres e
   endereço de loopback com porta não zero. Respostas também têm limite de
   4 KiB e rejeitam campos desconhecidos. Erros não reproduzem seu conteúdo.
7. A consulta HTTP tem timeout global de dois segundos, limite de 4 KiB para
   headers, sem redirects e sem proxies do ambiente. A trava também precisa
   ser arquivo regular, sem seguir links simbólicos.

## Origem e escopo

Melhoria operacional sem paridade com Koha, baseada na A3 da análise do painel
fornecida por Paulo. Reutiliza [saúde local](saude-local.md), ureq, serde_json e
tempfile existentes. Não acrescenta dependências nem versões de pacotes.
A observação descreve um instante, sem garantir disponibilidade futura.
Logs estruturados, IPC administrativo e controle de serviços ficam para
recortes posteriores. Outros comandos que ocupam a trava podem resultar em
`unavailable`, pois não oferecem saúde de servidor.

## Verificação

Testes cobrem servidor ativo, parada abrupta e normal, consulta sem efeitos
sobre os dados, caminhos relativos e precedência, conteúdo desconhecido,
identidade divergente, dados excessivos, endereço remoto, links, incoerência
entre status HTTP e componentes e proxies de ambiente.
