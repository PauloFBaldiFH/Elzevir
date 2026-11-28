# Endereços e acesso — P4

## 1. Comportamento

1. `[staff]` acrescenta três modos tipados ao TOML da A1. Ausência preserva o
   padrão `mode = "same_address"`: catálogo em `/`, equipe em `/equipe`.
2. `mode = "hostname"` exige `catalog_host` e `staff_host` DNS ASCII distintos,
   sem esquema, porta, caminho, wildcard ou credenciais. Nomes são normalizados
   para minúsculas; um ponto final DNS é aceito. No listener principal, todas
   as rotas da equipe exigem o Host da equipe; catálogo exige Host do catálogo.
   Assets estáticos funcionam nos dois. GET `/equipe` no catálogo redireciona
   para o endereço da equipe, sem servir login no catálogo; subrotas e POSTs
   continuam recusados. Links públicos no listener/host da equipe redirecionam
   para o catálogo. São redirects entre as portas realmente vinculadas, usando
   os nomes configurados ou IP específico vinculado; no bind wildcard por porta,
   usa o hostname validado do pedido,
   preservando HTTP/HTTPS do navegador. Não há redirect sem porta confirmada.
   Host desconhecido, repetido, inválido
   ou divergente de uma URI absoluta é recusado antes dos handlers.
3. `mode = "separate_listener"` exige `listen`; `allow_ips` é opcional, com no
   máximo 32 IPs ou redes CIDR IPv4/IPv6. O listener do catálogo recusa equipe;
   o da equipe encaminha navegação GET pública ao catálogo, mantendo assets.
   Não executa handlers do catálogo ali. Lista vazia não restringe IP.
   Lista não vazia exige o peer TCP real, nunca Forwarded/X-Forwarded-For.
   IPv4 mapeado em IPv6 é comparado como IPv4. Falta do peer recusa o acesso.
4. Ambos os listeners compartilham a mesma instituição, sessões e serviços;
   não abrem outra escritora. Falha ao vincular qualquer porta impede anúncio
   de prontidão. Ctrl+C encerra ambos, saúde e IPC. Cookies continuam sem Domain;
   autenticação, permissões e CSRF permanecem obrigatórios na equipe.
5. Configuração é validada antes de abrir dados. Campos incompatíveis com o
   modo e nomes/redes inválidos são recusados sem imprimir valores do TOML.
   Alteração é pelo TOML e requer reinício; esta entrega não oferece hot reload
   nem editor de configuração no painel. Os três modos e suas opções aparecem
   como alternativas na seção Endereços e acesso (`a`).
6. Metadata ganha `staff` opcional com modo e porta efetivamente vinculada.
   Status só o retorna após confirmar o servidor como na A4. Metadata antiga
   continua válida; ausência do campo é desconhecido, não inferido do TOML.
7. O painel separa a configuração carregada ao abrir, a execução confirmada e
   o resultado dos testes, incluindo horário UTC. Modo/porta da execução podem
   divergir do arquivo (CLI, arquivo alterado ou porta zero resolvida pelo SO).
8. URLs locais derivam dos listeners confirmados. Wildcards usam loopback;
   bindings específicos usam o IP vinculado. No modo hostname, URLs usam os
   nomes configurados e a porta HTTP observada, sem afirmar que DNS funciona.
   Todas as URLs geradas são HTTP direto; HTTPS/proxy/túnel vêm depois.
9. Alternativas de rede usam somente interfaces observadas pelo SO, com até
   oito IPs elegíveis por listener, da mesma família do bind. Excluem loopback,
   unspecified, multicast e link-local sem escopo. Bind loopback não gera LAN;
   bind específico só sugere seu próprio IP; wildcard permite IPs locais.
   Nomes de host exigem DNS/hosts no cliente e não geram URLs IP incompatíveis.
10. `v` na seção testa catálogo (`/`) e formulário de login (`/equipe/login`)
    em segundo plano, sem testar credenciais nem seguir redirects. Hostname é
    testado pelo IP local com Host explícito, sem consultar DNS. Cada requisição
    tem timeout de 500 ms, sem proxy, redirects nem leitura do corpo. HTTP 200
    é OK; outra resposta é falha. IP específico só é sondado se observado em
    uma interface local; falta de evidência é desconhecido. Não varre a rede.
11. Os testes da P3 também reutilizam essas sondas, eliminando duplicação.
    Testes da seção de endereços não executam `quick_check` nem exportam ZIP.
    Pedidos são coalescidos no worker existente; verificação/exportação incluem
    os testes de endereço quando pedidos juntos. Polls não refazem testes HTTP.
12. Mudança de identidade, listener, modo ou perda do servidor invalida os
    testes antigos. Observação expirada segue a P2 e oculta URLs antigas.
    Teste feito nesta máquina não comprova acesso de outro computador, DNS,
    firewall, TLS ou internet. Internet e túnel continuam não configurados.
13. `elzevir addresses --config ARQUIVO` (ou `--data-dir`) imprime JSON com os
    mesmos campos. `--test` executa sondas; sem ele é somente observação leve.
    Não cria instituição, não abre SQLite e não reescreve configuração.
14. UI Fluent en/pt-BR, ASCII, controles externos filtrados. Exportação de
    diagnóstico da P3 continua excluindo nomes, endereços e listas de IP.

## 2. Origem no Koha

Não porta comportamento do Koha. Base: doc 04 §5 e P4 da análise de Paulo.

## 3. Paridade e decisões

Livre. Regras de acesso ficam na camada web; TOML/listeners/sondas na borda do
binário; UI recebe apenas dados. Usa `sysinfo` já existente com feature network,
sem unsafe local nem dependência nova. Esta é a concretização HTTP da escolha
de três modos do doc 04; TLS e configuração de proxy são entregas posteriores.

## 4. Casos de borda

- Rotas GET/POST da equipe bloqueadas no catálogo; estáticos nos dois lados.
- Host falso/repetido e cabeçalhos de proxy não contornam a política.
- IP/CIDR inválido, limites de lista, redes v4/v6, binding em porta zero.
- Segunda porta ocupada não deixa serviço parcialmente anunciado.
- URLs IPv6 entre colchetes, DNS não comprovado, loopback sem alternativa LAN.
- Testes perdem validade após parada/reinício; modos antigos continuam válidos.
- CLI sem servidor não cria arquivos; tela e teclado têm as duas traduções.

## 5. Desvios

Nenhum: não há área de paridade.

## 6. Fora do escopo

Editor/instalador TUI, aplicar mudanças sem reiniciar, TLS embutido, proxy
confiável para recuperar IP original, DNS, firewall, abrir navegador/clipboard,
QR e publicação/túnel (P5–P7). Nomes de host separam roteamento, não substituem
autenticação; portas distintas no mesmo host não isolam cookies do navegador.

## 7. Central de endereços do painel (Marco E)

A visão geral oferece os endereços observados deste computador, da rede e
da internet, com ações para catálogo e equipe. Duas entradas de igual destaque
levam ao cadastro pelo serviço de endereços e à publicação com domínio próprio.
Escolher um percurso ou consultar resultados apenas muda a tela. A autorização,
a publicação e a revogação passam pelas operações e confirmações do host.

Na tela do serviço de endereços, o operador pode solicitar, retomar ou recuperar
o cadastro. O painel identifica fase, conector local, horário e atribuição
quando observados. Um cadastro ausente ou desconhecido não comprova que o
broker esteja online. No domínio próprio o operador autoriza a Cloudflare,
revisa os nomes de catálogo e equipe e só então solicita a configuração.

Testes e eventos têm entradas próprias. As sondas de loopback e as de HTTPS
público têm escopos e resultados identificados. Em 80×24 e em tela larga,
os mesmos percursos aceitam clique e Tab/Enter. O compartilhamento usa
somente a URL HTTPS de catálogo validada pelo contrato existente.
