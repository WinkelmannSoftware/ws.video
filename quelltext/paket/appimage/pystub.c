/* Platzhalter für libpython im AppImage.
 * Die Schnittbibliothek GES bindet Python ein, um zusätzliche Projektformate zu laden, die WS.Video nicht braucht.
 * Ohne die Python-Standardbibliothek des Bausystems bräche das echte Python den Programmstart ab.
 * Dieser Platzhalter meldet „kein Python“, GES überspringt den Schritt dann von selbst. */
#include <stddef.h>
long _Py_NoneStruct[4];
int Py_IsInitialized(void) { return 0; }
void Py_Initialize(void) {}
void *PyImport_AddModule(const char *name) { (void)name; return NULL; }
void *PyModule_GetDict(void *m) { (void)m; return NULL; }
void *Py_CompileStringExFlags(const char *s, const char *f, int a, void *b, int c) { (void)s; (void)f; (void)a; (void)b; (void)c; return NULL; }
void *PyEval_EvalCode(void *a, void *b, void *c) { (void)a; (void)b; (void)c; return NULL; }
void *PyEval_SaveThread(void) { return NULL; }
int PyGILState_Ensure(void) { return 0; }
void PyGILState_Release(int s) { (void)s; }
void PyErr_Clear(void) {}
void PyErr_Print(void) {}
void *PyErr_Occurred(void) { return NULL; }
void PyErr_Fetch(void **a, void **b, void **c) { if (a) *a = NULL; if (b) *b = NULL; if (c) *c = NULL; }
void PyErr_NormalizeException(void **a, void **b, void **c) { (void)a; (void)b; (void)c; }
void *PyObject_Repr(void *o) { (void)o; return NULL; }
void *PyUnicode_AsEncodedString(void *o, const char *e, const char *r) { (void)o; (void)e; (void)r; return NULL; }
void _Py_Dealloc(void *o) { (void)o; }
