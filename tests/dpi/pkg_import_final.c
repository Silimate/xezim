/* A package's DPI imports called as `pkg::f()` from a final procedure
 * (IEEE 1800 §9.2.3, §26.3, §35.5.4). */
static int calls;
void pif_mark(void) { calls++; }
int pif_calls(void) { return calls; }
